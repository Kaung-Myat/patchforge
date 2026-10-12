import 'dart:io';

import 'package:crypto/crypto.dart';
import 'package:flutter/widgets.dart';

import 'client.dart';
import 'config.dart';
import 'delta/bsdiff_patcher.dart';
import 'exceptions/patchforge_exception.dart';
import 'models/boot_confirmation_result.dart';
import 'models/patch_state.dart';
import 'models/update_result.dart';
import 'platform/app_metadata.dart';
import 'platform/base_artifact_locator.dart';
import 'security/signature_verifier.dart';
import 'storage/patch_storage.dart';

class PatchForge {
  final PatchForgeConfig config;

  late final PatchForgeClient _client;
  late final PatchStorage _storage;
  late final AppMetadataReader _metadataReader;
  late final BaseArtifactLocator _baseArtifactLocator;

  /// Completes after startup determines whether a patched boot needs to be
  /// confirmed, rolled back, or left unchanged.
  late final Future<PatchBootConfirmationResult> bootConfirmation;

  PatchForge({required this.config}) {
    _client = PatchForgeClient(baseUrl: config.baseUrl, appId: config.appId);

    _storage = PatchStorage();

    _metadataReader = AppMetadataReader();

    _baseArtifactLocator = BaseArtifactLocator();

    bootConfirmation = _confirmBootWhenHealthy();
  }

  Future<PatchBootConfirmationResult> _confirmBootWhenHealthy() async {
    var state = await getState();

    if (state.status != PatchStateStatus.booting ||
        state.pendingPatch == null) {
      return const PatchBootConfirmationResult(
        status: PatchBootConfirmationStatus.notRequired,
      );
    }

    final pendingPatch = state.pendingPatch!;

    // Reaching the end of a Flutter frame proves that the patched isolate can
    // initialize and render before its boot is accepted.
    await WidgetsBinding.instance.endOfFrame;
    await Future<void>.delayed(config.bootStabilizationDelay);

    state = await getState();

    if (state.status != PatchStateStatus.booting ||
        state.pendingPatch != pendingPatch) {
      return PatchBootConfirmationResult(
        status: PatchBootConfirmationStatus.failed,
        patchNumber: pendingPatch,
        message: 'Patch boot state changed before confirmation.',
      );
    }

    if (state.loadedPatch != pendingPatch) {
      await rollback();

      return PatchBootConfirmationResult(
        status: PatchBootConfirmationStatus.rolledBack,
        patchNumber: pendingPatch,
        message: 'The pending patch was not loaded by the Flutter engine.',
      );
    }

    try {
      final healthCheck = config.bootHealthCheck;
      final healthy = healthCheck == null || await healthCheck();

      if (!healthy) {
        await rollback();

        return PatchBootConfirmationResult(
          status: PatchBootConfirmationStatus.rolledBack,
          patchNumber: pendingPatch,
          message: 'The application health check failed.',
        );
      }

      await markPatchSuccessful(pendingPatch);

      return PatchBootConfirmationResult(
        status: PatchBootConfirmationStatus.confirmed,
        patchNumber: pendingPatch,
      );
    } catch (error) {
      await rollback();

      return PatchBootConfirmationResult(
        status: PatchBootConfirmationStatus.rolledBack,
        patchNumber: pendingPatch,
        message: 'Boot confirmation failed: $error',
      );
    }
  }

  Future<PatchUpdateResult> update() async {
    final metadata = await _readMetadata();

    final currentPatch = await _storage.getCurrentPatch();
    final requestedArchitecture = await _resolveArchitecture();
    final deviceId = await _storage.getOrCreateDeviceId();

    final update = await _client.checkForUpdate(
      version: metadata.version,
      platform: metadata.platform,
      architecture: requestedArchitecture,
      channel: config.channel,
      deviceId: deviceId,
      currentPatch: currentPatch,
    );

    if (!update.updateAvailable || update.manifest == null) {
      return PatchUpdateResult(
        status: PatchUpdateStatus.noUpdate,
        currentPatch: currentPatch,
      );
    }

    final manifest = update.manifest!;

    // Manifest protocol version
    if (manifest.manifestVersion != 1 && manifest.manifestVersion != 2) {
      throw PatchForgeException(
        code: PatchForgeErrorCode.invalidResponse,
        message:
            'Unsupported manifest version: '
            '${manifest.manifestVersion}',
      );
    }

    // Manifest must belong to this app
    if (manifest.appId != config.appId) {
      throw const PatchForgeException(
        code: PatchForgeErrorCode.invalidResponse,
        message: 'Manifest app ID mismatch.',
      );
    }

    // Manifest must match installed app version
    if (manifest.version != metadata.version) {
      throw const PatchForgeException(
        code: PatchForgeErrorCode.invalidResponse,
        message: 'Manifest version mismatch.',
      );
    }

    // Manifest must match current platform
    if (manifest.platform != metadata.platform) {
      throw const PatchForgeException(
        code: PatchForgeErrorCode.invalidResponse,
        message: 'Manifest platform mismatch.',
      );
    }

    if (manifest.channel != config.channel) {
      throw PatchForgeException(
        code: PatchForgeErrorCode.invalidResponse,
        message:
            'Manifest channel mismatch. Expected ${config.channel}, '
            'received ${manifest.channel}.',
      );
    }

    if (manifest.rolloutPercentage < 0 || manifest.rolloutPercentage > 100) {
      throw const PatchForgeException(
        code: PatchForgeErrorCode.invalidResponse,
        message: 'Manifest rollout percentage is invalid.',
      );
    }

    if (manifest.architecture != requestedArchitecture) {
      throw PatchForgeException(
        code: PatchForgeErrorCode.invalidResponse,
        message:
            'Manifest architecture mismatch. Expected $requestedArchitecture, '
            'received ${manifest.architecture ?? 'none'}.',
      );
    }

    if (manifest.basePatchNumber != null &&
        manifest.basePatchNumber != currentPatch) {
      throw PatchForgeException(
        code: PatchForgeErrorCode.invalidResponse,
        message:
            'Incompatible patch chain. Current patch is $currentPatch, '
            'but manifest requires ${manifest.basePatchNumber}.',
      );
    }

    // Prevent accidental downgrade/replay
    if (manifest.patchNumber <= currentPatch) {
      throw PatchForgeException(
        code: PatchForgeErrorCode.invalidResponse,
        message:
            'Manifest patch number '
            '${manifest.patchNumber} '
            'is not newer than '
            '$currentPatch.',
      );
    }

    // Find trusted public key by key_id
    if (config.revokedKeyIds.contains(manifest.keyId)) {
      throw PatchForgeException(
        code: PatchForgeErrorCode.signature,
        message: 'Signing key has been revoked: ${manifest.keyId}',
      );
    }

    final publicKey = config.trustedPublicKeys[manifest.keyId];

    if (publicKey == null) {
      throw PatchForgeException(
        code: PatchForgeErrorCode.signature,
        message:
            'Unknown signing key: '
            '${manifest.keyId}',
      );
    }

    final verifier = PatchSignatureVerifier(publicKeyBase64: publicKey);

    // Verify manifest authenticity
    final signatureValid = await verifier.verify(
      manifestVersion: manifest.manifestVersion,

      keyId: manifest.keyId,

      appId: manifest.appId,

      releaseId: manifest.releaseId,

      version: manifest.version,

      platform: manifest.platform,

      patchNumber: manifest.patchNumber,

      artifactPath: manifest.artifactPath,

      sha256: manifest.sha256,

      fileSize: manifest.fileSize,

      patchFormat: manifest.patchFormat,

      architecture: manifest.architecture,

      baseSha256: manifest.baseSha256,

      basePatchNumber: manifest.basePatchNumber,

      targetSha256: manifest.targetSha256,

      targetFileSize: manifest.targetFileSize,

      channel: manifest.channel,

      rolloutPercentage: manifest.rolloutPercentage,

      signatureBase64: manifest.signature,
    );

    if (!signatureValid) {
      throw const PatchForgeException(
        code: PatchForgeErrorCode.signature,
        message: 'Patch signature verification failed.',
      );
    }

    // Signature passed.
    // Now download + verify file bytes.
    final patchFile = await _client.downloadPatch(manifest);

    final patchFormat = manifest.patchFormat;
    final architecture = manifest.architecture;
    final baseSha256 = manifest.baseSha256;
    final targetSha256 = manifest.targetSha256;
    final targetFileSize = manifest.targetFileSize;

    if (patchFormat != 'bsdiff' ||
        architecture == null ||
        baseSha256 == null ||
        targetSha256 == null ||
        targetFileSize == null) {
      throw const PatchForgeException(
        code: PatchForgeErrorCode.invalidResponse,
        message: 'Manifest does not contain complete BSDIFF metadata.',
      );
    }

    final baseFile = await _resolveBaseArtifact(
      currentPatch: currentPatch,
      architecture: architecture,
    );
    final baseBytes = await baseFile.readAsBytes();
    final actualBaseSha256 = sha256.convert(baseBytes).toString();

    if (actualBaseSha256 != baseSha256) {
      throw PatchForgeException(
        code: PatchForgeErrorCode.integrity,
        message:
            'Base libapp.so SHA-256 mismatch. '
            'Expected $baseSha256, received $actualBaseSha256.',
      );
    }

    late final List<int> targetBytes;

    try {
      targetBytes = BsdiffPatcher().apply(
        oldBytes: baseBytes,
        patchBytes: await patchFile.readAsBytes(),
      );
    } on BsdiffException catch (error) {
      throw PatchForgeException(
        code: PatchForgeErrorCode.integrity,
        message: 'Failed to apply BSDIFF patch: ${error.message}',
      );
    }

    if (targetBytes.length != targetFileSize) {
      throw PatchForgeException(
        code: PatchForgeErrorCode.integrity,
        message:
            'Reconstructed libapp.so size mismatch. '
            'Expected $targetFileSize, received ${targetBytes.length}.',
      );
    }

    final actualTargetSha256 = sha256.convert(targetBytes).toString();

    if (actualTargetSha256 != targetSha256) {
      throw PatchForgeException(
        code: PatchForgeErrorCode.integrity,
        message:
            'Reconstructed libapp.so SHA-256 mismatch. '
            'Expected $targetSha256, received $actualTargetSha256.',
      );
    }

    final reconstructedFile = await _storage.saveReconstructedArtifact(
      patchNumber: manifest.patchNumber,
      bytes: targetBytes,
    );

    // Verified and reconstructed, but not active until a later boot succeeds.
    await _storage.stagePatch(
      patchNumber: manifest.patchNumber,
      targetSha256: targetSha256,
      targetFileSize: targetFileSize,
    );

    return PatchUpdateResult(
      status: PatchUpdateStatus.staged,

      currentPatch: currentPatch,

      newPatch: manifest.patchNumber,

      file: reconstructedFile,

      patchFile: patchFile,
    );
  }

  Future<File> _resolveBaseArtifact({
    required int currentPatch,
    required String architecture,
  }) async {
    final stored = await _storage.getReconstructedArtifact(currentPatch);

    if (stored != null) {
      return stored;
    }

    final provider = config.baseArtifactProvider;

    try {
      return provider != null
          ? await provider(architecture)
          : await _baseArtifactLocator.locate(architecture);
    } catch (error) {
      throw PatchForgeException(
        code: PatchForgeErrorCode.unsupportedPlatform,
        message: 'Failed to locate the base libapp.so: $error',
      );
    }
  }

  Future<String> _resolveArchitecture() async {
    final configured = config.architecture;

    if (configured != null && configured.isNotEmpty) {
      return configured;
    }

    try {
      return await _baseArtifactLocator.getArchitecture();
    } catch (error) {
      throw PatchForgeException(
        code: PatchForgeErrorCode.unsupportedPlatform,
        message: 'Failed to determine the device architecture: $error',
      );
    }
  }

  Future<void> markPatchSuccessful(int patchNumber) async {
    final loadedPatch = await _baseArtifactLocator.getLoadedPatch();

    if (loadedPatch != patchNumber) {
      throw PatchForgeException(
        code: PatchForgeErrorCode.invalidResponse,
        message:
            'Cannot commit patch $patchNumber because the running engine '
            'loaded patch ${loadedPatch ?? 'none'}.',
      );
    }

    await _storage.commitPatch(patchNumber);
  }

  Future<void> rollback() async {
    await _storage.rollbackPatch();
  }

  Future<PatchState> getState() async {
    final state = await _storage.getPatchState();

    try {
      return state.withLoadedPatch(await _baseArtifactLocator.getLoadedPatch());
    } catch (_) {
      return state;
    }
  }

  Future<AppMetadata> _readMetadata() async {
    try {
      return await _metadataReader.read();
    } on UnsupportedError catch (error) {
      throw PatchForgeException(
        code: PatchForgeErrorCode.unsupportedPlatform,
        message: error.message?.toString() ?? 'Unsupported platform.',
      );
    }
  }
}

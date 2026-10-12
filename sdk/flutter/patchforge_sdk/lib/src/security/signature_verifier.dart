import 'dart:convert';

import 'package:cryptography/cryptography.dart';

class PatchSignatureVerifier {
  final String publicKeyBase64;

  const PatchSignatureVerifier({required this.publicKeyBase64});

  String buildPayload({
    required int manifestVersion,
    required String keyId,
    required String appId,
    required String releaseId,
    required String version,
    required String platform,
    required int patchNumber,
    required String artifactPath,
    required String sha256,
    required int fileSize,
    String? patchFormat,
    String? architecture,
    String? baseSha256,
    int? basePatchNumber,
    String? targetSha256,
    int? targetFileSize,
    String channel = 'stable',
    int rolloutPercentage = 100,
  }) {
    final fields = [
      'patchforge-manifest',
      manifestVersion.toString(),
      keyId,
      appId,
      releaseId,
      version,
      platform,
      patchNumber.toString(),
      artifactPath,
      sha256,
      fileSize.toString(),
      patchFormat ?? '',
      architecture ?? '',
      baseSha256 ?? '',
      targetSha256 ?? '',
      targetFileSize?.toString() ?? '',
    ];
    if (manifestVersion >= 2) {
      fields.addAll([
        basePatchNumber?.toString() ?? '',
        channel,
        rolloutPercentage.toString(),
      ]);
    }
    return fields.join('\n');
  }

  Future<bool> verify({
    required int manifestVersion,
    required String keyId,
    required String appId,
    required String releaseId,
    required String version,
    required String platform,
    required int patchNumber,
    required String artifactPath,
    required String sha256,
    required int fileSize,
    String? patchFormat,
    String? architecture,
    String? baseSha256,
    int? basePatchNumber,
    String? targetSha256,
    int? targetFileSize,
    String channel = 'stable',
    int rolloutPercentage = 100,
    required String signatureBase64,
  }) async {
    final payload = buildPayload(
      manifestVersion: manifestVersion,
      keyId: keyId,
      appId: appId,
      releaseId: releaseId,
      version: version,
      platform: platform,
      patchNumber: patchNumber,
      artifactPath: artifactPath,
      sha256: sha256,
      fileSize: fileSize,
      patchFormat: patchFormat,
      architecture: architecture,
      baseSha256: baseSha256,
      basePatchNumber: basePatchNumber,
      targetSha256: targetSha256,
      targetFileSize: targetFileSize,
      channel: channel,
      rolloutPercentage: rolloutPercentage,
    );

    final publicKey = SimplePublicKey(
      base64Decode(publicKeyBase64),
      type: KeyPairType.ed25519,
    );

    final signature = Signature(
      base64Decode(signatureBase64),
      publicKey: publicKey,
    );

    return Ed25519().verify(utf8.encode(payload), signature: signature);
  }
}

import 'dart:io';
import 'dart:math';

import 'package:path_provider/path_provider.dart';
import 'package:shared_preferences/shared_preferences.dart';

import '../models/patch_state.dart';

class PatchStorage {
  static const _currentPatchKey = 'patchforge_current_patch';

  static const _previousPatchKey = 'patchforge_previous_patch';

  static const _pendingPatchKey = 'patchforge_pending_patch';

  static const _patchStatusKey = 'patchforge_patch_status';
  static const _deviceIdKey = 'patchforge_device_id';

  Future<String> getOrCreateDeviceId() async {
    final prefs = await SharedPreferences.getInstance();
    final existing = prefs.getString(_deviceIdKey);
    if (existing != null && existing.isNotEmpty) {
      return existing;
    }

    final random = Random.secure();
    final id = List<int>.generate(
      16,
      (_) => random.nextInt(256),
    ).map((byte) => byte.toRadixString(16).padLeft(2, '0')).join();
    await prefs.setString(_deviceIdKey, id);
    return id;
  }

  static String _patchSha256Key(int patchNumber) =>
      'patchforge_patch_${patchNumber}_sha256';

  static String _patchSizeKey(int patchNumber) =>
      'patchforge_patch_${patchNumber}_size';

  Future<Directory> getPatchDirectory() async {
    // Flutter accepts custom AOT libraries only below Android's filesDir.
    final appDir = await getApplicationSupportDirectory();

    final patchDir = Directory('${appDir.path}/patchforge/patches');

    if (!await patchDir.exists()) {
      await patchDir.create(recursive: true);
    }

    return patchDir;
  }

  Future<File> savePatch({
    required int patchNumber,
    required List<int> bytes,
  }) async {
    final directory = await getPatchDirectory();

    final file = File('${directory.path}/patch-$patchNumber.bin');

    await file.writeAsBytes(bytes);

    return file;
  }

  Future<File> saveReconstructedArtifact({
    required int patchNumber,
    required List<int> bytes,
  }) async {
    final patchDirectory = await _getPatchVersionDirectory(patchNumber);
    final destination = File('${patchDirectory.path}/libapp.so');
    final temporary = File('${destination.path}.tmp');

    await temporary.writeAsBytes(bytes, flush: true);

    if (await destination.exists()) {
      await destination.delete();
    }

    return temporary.rename(destination.path);
  }

  Future<File?> getReconstructedArtifact(int patchNumber) async {
    if (patchNumber <= 0) {
      return null;
    }

    final directory = await getPatchDirectory();
    final file = File('${directory.path}/patch-$patchNumber/libapp.so');

    return await file.exists() ? file : null;
  }

  Future<Directory> _getPatchVersionDirectory(int patchNumber) async {
    final directory = await getPatchDirectory();
    final patchDirectory = Directory('${directory.path}/patch-$patchNumber');

    if (!await patchDirectory.exists()) {
      await patchDirectory.create(recursive: true);
    }

    return patchDirectory;
  }

  Future<void> saveCurrentPatch(int patchNumber) async {
    final prefs = await SharedPreferences.getInstance();

    await prefs.setInt(_currentPatchKey, patchNumber);
  }

  Future<int> getCurrentPatch() async {
    final prefs = await SharedPreferences.getInstance();

    return prefs.getInt(_currentPatchKey) ?? 0;
  }

  Future<void> stagePatch({
    required int patchNumber,
    required String targetSha256,
    required int targetFileSize,
  }) async {
    final prefs = await SharedPreferences.getInstance();

    final currentPatch = prefs.getInt(_currentPatchKey) ?? 0;

    await prefs.setInt(_previousPatchKey, currentPatch);

    await prefs.setInt(_pendingPatchKey, patchNumber);

    await prefs.setString(_patchSha256Key(patchNumber), targetSha256);

    await prefs.setInt(_patchSizeKey(patchNumber), targetFileSize);

    await prefs.setString(_patchStatusKey, 'staged');
  }

  Future<void> commitPatch(int patchNumber) async {
    final prefs = await SharedPreferences.getInstance();

    await prefs.setInt(_currentPatchKey, patchNumber);

    await prefs.remove(_pendingPatchKey);

    await prefs.setString(_patchStatusKey, 'active');
  }

  Future<void> rollbackPatch() async {
    final prefs = await SharedPreferences.getInstance();

    final previousPatch = prefs.getInt(_previousPatchKey) ?? 0;
    final pendingPatch = prefs.getInt(_pendingPatchKey);

    await prefs.setInt(_currentPatchKey, previousPatch);

    await prefs.remove(_pendingPatchKey);

    await prefs.setString(_patchStatusKey, 'failed');

    if (pendingPatch != null) {
      await prefs.remove(_patchSha256Key(pendingPatch));

      await prefs.remove(_patchSizeKey(pendingPatch));

      final directory = await getPatchDirectory();
      final pendingDirectory = Directory(
        '${directory.path}/patch-$pendingPatch',
      );

      if (await pendingDirectory.exists()) {
        await pendingDirectory.delete(recursive: true);
      }
    }
  }

  Future<PatchState> getPatchState() async {
    final prefs = await SharedPreferences.getInstance();

    final current = prefs.getInt(_currentPatchKey) ?? 0;
    final previous = prefs.getInt(_previousPatchKey);
    final pending = prefs.getInt(_pendingPatchKey);

    final statusString = prefs.getString(_patchStatusKey);

    final status = switch (statusString) {
      'staged' => PatchStateStatus.staged,
      'booting' => PatchStateStatus.booting,
      'active' => PatchStateStatus.active,
      'failed' => PatchStateStatus.failed,

      // If we already have a committed patch but old installs
      // don't have a stored status yet, treat it as active.
      _ => current > 0 ? PatchStateStatus.active : PatchStateStatus.none,
    };

    return PatchState(
      currentPatch: current,
      previousPatch: previous,
      pendingPatch: pending,
      status: status,
    );
  }
}

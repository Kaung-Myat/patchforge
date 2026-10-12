import 'dart:io';

enum PatchUpdateStatus { noUpdate, downloaded, staged }

class PatchUpdateResult {
  final PatchUpdateStatus status;
  final int currentPatch;
  final int? newPatch;

  /// The verified reconstructed libapp.so for delta updates.
  final File? file;

  /// The downloaded and hash-verified delta artifact.
  final File? patchFile;

  const PatchUpdateResult({
    required this.status,
    required this.currentPatch,
    this.newPatch,
    this.file,
    this.patchFile,
  });

  bool get updated => status != PatchUpdateStatus.noUpdate;
}

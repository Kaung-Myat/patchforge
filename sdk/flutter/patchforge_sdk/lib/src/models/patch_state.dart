enum PatchStateStatus { none, staged, booting, active, failed }

class PatchState {
  final int currentPatch;
  final int? previousPatch;
  final int? pendingPatch;
  final int? loadedPatch;
  final PatchStateStatus status;

  const PatchState({
    required this.currentPatch,
    required this.previousPatch,
    required this.pendingPatch,
    required this.status,
    this.loadedPatch,
  });

  PatchState withLoadedPatch(int? patchNumber) {
    return PatchState(
      currentPatch: currentPatch,
      previousPatch: previousPatch,
      pendingPatch: pendingPatch,
      status: status,
      loadedPatch: patchNumber,
    );
  }
}

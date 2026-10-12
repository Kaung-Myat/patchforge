enum PatchBootConfirmationStatus { notRequired, confirmed, rolledBack, failed }

class PatchBootConfirmationResult {
  final PatchBootConfirmationStatus status;
  final int? patchNumber;
  final String? message;

  const PatchBootConfirmationResult({
    required this.status,
    this.patchNumber,
    this.message,
  });

  bool get confirmed => status == PatchBootConfirmationStatus.confirmed;
}

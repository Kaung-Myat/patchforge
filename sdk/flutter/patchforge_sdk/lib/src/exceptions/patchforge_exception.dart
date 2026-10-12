enum PatchForgeErrorCode {
  network,
  server,
  invalidResponse,
  integrity,
  signature,
  unsupportedPlatform,
}

class PatchForgeException implements Exception {
  final PatchForgeErrorCode code;
  final String message;

  const PatchForgeException({required this.code, required this.message});

  @override
  String toString() {
    return 'PatchForgeException($code): $message';
  }
}

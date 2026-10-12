import 'dart:async';
import 'dart:io';

typedef PatchForgeBaseArtifactProvider =
    Future<File> Function(String architecture);

typedef PatchForgeBootHealthCheck = FutureOr<bool> Function();

class PatchForgeConfig {
  final String baseUrl;
  final String appId;

  /// Release channel used for update checks.
  final String channel;

  /// Overrides automatic device architecture discovery.
  final String? architecture;

  final Map<String, String> trustedPublicKeys;

  /// Key IDs that must be rejected even if their public keys remain bundled.
  final Set<String> revokedKeyIds;

  /// Overrides Android's installed libapp.so discovery. Useful for tests and
  /// custom embedders.
  final PatchForgeBaseArtifactProvider? baseArtifactProvider;

  /// Time the patched app must remain responsive after its first frame before
  /// the boot can be committed.
  final Duration bootStabilizationDelay;

  /// Optional application-specific health check run before committing a boot.
  final PatchForgeBootHealthCheck? bootHealthCheck;

  const PatchForgeConfig({
    required this.baseUrl,
    required this.appId,
    required this.trustedPublicKeys,
    this.revokedKeyIds = const {},
    this.channel = 'stable',
    this.architecture,
    this.baseArtifactProvider,
    this.bootStabilizationDelay = const Duration(seconds: 5),
    this.bootHealthCheck,
  }) : assert(
         channel == 'development' ||
             channel == 'staging' ||
             channel == 'beta' ||
             channel == 'stable',
         'Unsupported PatchForge release channel.',
       );
}

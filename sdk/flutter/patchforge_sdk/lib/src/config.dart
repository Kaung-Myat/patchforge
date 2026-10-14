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

  /// Creates the SDK configuration injected automatically by the PatchForge
  /// CLI during `patchforge release` and `patchforge patch` builds.
  factory PatchForgeConfig.fromEnvironment({
    Set<String> revokedKeyIds = const {},
    String? architecture,
    PatchForgeBaseArtifactProvider? baseArtifactProvider,
    Duration bootStabilizationDelay = const Duration(seconds: 5),
    PatchForgeBootHealthCheck? bootHealthCheck,
  }) {
    const baseUrl = String.fromEnvironment('PATCHFORGE_URL');
    const appId = String.fromEnvironment('PATCHFORGE_APP_ID');
    const channel = String.fromEnvironment(
      'PATCHFORGE_CHANNEL',
      defaultValue: 'stable',
    );
    const signingKeyId = String.fromEnvironment('PATCHFORGE_KEY_ID');
    const signingPublicKey = String.fromEnvironment('PATCHFORGE_PUBLIC_KEY');

    if (baseUrl.isEmpty ||
        appId.isEmpty ||
        signingKeyId.isEmpty ||
        signingPublicKey.isEmpty) {
      throw StateError(
        'PatchForge build configuration is missing. Build with '
        '`patchforge release` or `patchforge patch`.',
      );
    }

    return PatchForgeConfig(
      baseUrl: baseUrl,
      appId: appId,
      channel: channel,
      trustedPublicKeys: {signingKeyId: signingPublicKey},
      revokedKeyIds: revokedKeyIds,
      architecture: architecture,
      baseArtifactProvider: baseArtifactProvider,
      bootStabilizationDelay: bootStabilizationDelay,
      bootHealthCheck: bootHealthCheck,
    );
  }
}

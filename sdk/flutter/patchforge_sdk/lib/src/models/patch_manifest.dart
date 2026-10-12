class PatchManifest {
  final int manifestVersion;
  final String keyId;

  final String appId;
  final String releaseId;

  final String version;
  final String platform;
  final String channel;

  final int patchNumber;

  final String artifactPath;

  final String sha256;
  final int fileSize;

  final String? patchFormat;
  final String? architecture;
  final String? baseSha256;
  final int? basePatchNumber;
  final String? targetSha256;
  final int? targetFileSize;
  final int rolloutPercentage;

  final String signature;

  const PatchManifest({
    required this.manifestVersion,
    required this.keyId,
    required this.appId,
    required this.releaseId,
    required this.version,
    required this.platform,
    required this.channel,
    required this.patchNumber,
    required this.artifactPath,
    required this.sha256,
    required this.fileSize,
    this.patchFormat,
    this.architecture,
    this.baseSha256,
    this.basePatchNumber,
    this.targetSha256,
    this.targetFileSize,
    required this.rolloutPercentage,
    required this.signature,
  });

  factory PatchManifest.fromJson(Map<String, dynamic> json) {
    return PatchManifest(
      manifestVersion: json['manifest_version'] as int,

      keyId: json['key_id'] as String,

      appId: json['app_id'] as String,

      releaseId: json['release_id'] as String,

      version: json['version'] as String,

      platform: json['platform'] as String,

      channel: json['channel'] as String? ?? 'stable',

      patchNumber: json['patch_number'] as int,

      artifactPath: json['artifact_path'] as String,

      sha256: json['sha256'] as String,

      fileSize: json['file_size'] as int,

      patchFormat: json['patch_format'] as String?,

      architecture: json['architecture'] as String?,

      baseSha256: json['base_sha256'] as String?,

      basePatchNumber: json['base_patch_number'] as int?,

      targetSha256: json['target_sha256'] as String?,

      targetFileSize: json['target_file_size'] as int?,

      rolloutPercentage: json['rollout_percentage'] as int? ?? 100,

      signature: json['signature'] as String,
    );
  }
}

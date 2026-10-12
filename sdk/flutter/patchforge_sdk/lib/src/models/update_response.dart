import 'patch_manifest.dart';

class UpdateResponse {
  final bool updateAvailable;
  final PatchManifest? manifest;

  const UpdateResponse({required this.updateAvailable, required this.manifest});

  factory UpdateResponse.fromJson(Map<String, dynamic> json) {
    return UpdateResponse(
      updateAvailable: json['update_available'] as bool,
      manifest: json['manifest'] != null
          ? PatchManifest.fromJson(json['manifest'] as Map<String, dynamic>)
          : null,
    );
  }
}

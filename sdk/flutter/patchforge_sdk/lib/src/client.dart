import 'dart:convert';
import 'dart:io';

import 'package:crypto/crypto.dart';
import 'package:http/http.dart' as http;

import 'exceptions/patchforge_exception.dart';
import 'models/patch_manifest.dart';
import 'models/update_response.dart';
import 'storage/patch_storage.dart';

Uri buildPatchDownloadUri({
  required String baseUrl,
  required String artifactPath,
}) {
  final trimmedPath = artifactPath.trim();

  if (trimmedPath.isEmpty) {
    throw const FormatException('Patch artifact path cannot be empty.');
  }

  final parsedPath = Uri.parse(trimmedPath);

  if (parsedPath.hasScheme || parsedPath.hasAuthority) {
    throw const FormatException(
      'Patch artifact path must be relative to the PatchForge server.',
    );
  }

  final pathWithoutLeadingSlash = parsedPath.path.replaceFirst(
    RegExp(r'^/+'),
    '',
  );
  final serverPath = pathWithoutLeadingSlash.startsWith('artifacts/')
      ? '/$pathWithoutLeadingSlash'
      : '/artifacts/$pathWithoutLeadingSlash';

  return Uri.parse(baseUrl).resolve(serverPath);
}

class PatchForgeClient {
  final String baseUrl;
  final String appId;

  const PatchForgeClient({required this.baseUrl, required this.appId});

  Future<UpdateResponse> checkForUpdate({
    required String version,
    required String platform,
    required String architecture,
    required String channel,
    required String deviceId,
    int currentPatch = 0,
  }) async {
    final uri = Uri.parse('$baseUrl/api/v1/apps/$appId/updates').replace(
      queryParameters: {
        'version': version,
        'platform': platform,
        'architecture': architecture,
        'channel': channel,
        'device_id': deviceId,
        'patch': currentPatch.toString(),
      },
    );

    late http.Response response;

    try {
      response = await http.get(uri).timeout(const Duration(seconds: 10));
    } catch (error) {
      throw PatchForgeException(
        code: PatchForgeErrorCode.network,
        message: 'Failed to connect to PatchForge server: $error',
      );
    }

    if (response.statusCode != 200) {
      throw PatchForgeException(
        code: PatchForgeErrorCode.server,
        message:
            'Server returned ${response.statusCode}: '
            '${response.body}',
      );
    }

    try {
      final json = jsonDecode(response.body) as Map<String, dynamic>;

      return UpdateResponse.fromJson(json);
    } catch (error) {
      throw PatchForgeException(
        code: PatchForgeErrorCode.invalidResponse,
        message: 'Invalid server response: $error',
      );
    }
  }

  Future<File> downloadPatch(PatchManifest manifest) async {
    late final Uri downloadUri;

    try {
      downloadUri = buildPatchDownloadUri(
        baseUrl: baseUrl,
        artifactPath: manifest.artifactPath,
      );
    } on FormatException catch (error) {
      throw PatchForgeException(
        code: PatchForgeErrorCode.invalidResponse,
        message: 'Invalid patch artifact path: ${error.message}',
      );
    }

    late http.Response response;

    try {
      response = await http
          .get(downloadUri)
          .timeout(const Duration(seconds: 30));
    } catch (error) {
      throw PatchForgeException(
        code: PatchForgeErrorCode.network,
        message: 'Failed to download patch: $error',
      );
    }

    if (response.statusCode != 200) {
      throw PatchForgeException(
        code: PatchForgeErrorCode.server,
        message:
            'Patch download failed: '
            '${response.statusCode} ($downloadUri)',
      );
    }

    final bytes = response.bodyBytes;

    if (bytes.length != manifest.fileSize) {
      throw PatchForgeException(
        code: PatchForgeErrorCode.integrity,
        message:
            'Patch size mismatch. '
            'Expected ${manifest.fileSize}, '
            'received ${bytes.length}.',
      );
    }

    final calculatedHash = sha256.convert(bytes).toString();

    if (calculatedHash != manifest.sha256) {
      throw const PatchForgeException(
        code: PatchForgeErrorCode.integrity,
        message: 'Patch SHA-256 verification failed.',
      );
    }

    final storage = PatchStorage();

    final file = await storage.savePatch(
      patchNumber: manifest.patchNumber,
      bytes: bytes,
    );

    return file;
  }
}

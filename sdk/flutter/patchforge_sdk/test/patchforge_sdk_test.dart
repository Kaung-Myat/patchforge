import 'dart:convert';
import 'dart:io';

import 'package:crypto/crypto.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:patchforge_sdk/src/config.dart';
import 'package:patchforge_sdk/src/client.dart';
import 'package:patchforge_sdk/src/delta/bsdiff_patcher.dart';
import 'package:patchforge_sdk/src/models/patch_manifest.dart';
import 'package:patchforge_sdk/src/platform/base_artifact_locator.dart';
import 'package:patchforge_sdk/src/security/signature_verifier.dart';

void main() {
  test('Android plugin declares the Internet permission for release apps', () {
    final manifest = File(
      'android/src/main/AndroidManifest.xml',
    ).readAsStringSync();

    expect(
      manifest,
      contains('android.permission.INTERNET'),
      reason: 'Release Flutter apps do not inherit the debug-only permission.',
    );
  });

  test('environment configuration fails clearly without CLI build values', () {
    expect(PatchForgeConfig.fromEnvironment, throwsStateError);
  });

  group('ELF architecture detection', () {
    List<int> elfHeader(int machine) {
      final header = List<int>.filled(20, 0);
      header.setAll(0, [0x7f, 0x45, 0x4c, 0x46]);
      header[5] = 1;
      header[18] = machine & 0xff;
      header[19] = machine >> 8;
      return header;
    }

    test('maps Android ELF machine types to ABI names', () {
      expect(architectureFromElfHeader(elfHeader(183)), 'arm64-v8a');
      expect(architectureFromElfHeader(elfHeader(40)), 'armeabi-v7a');
      expect(architectureFromElfHeader(elfHeader(62)), 'x86_64');
    });

    test('rejects invalid and unsupported ELF headers', () {
      expect(
        () => architectureFromElfHeader(List.filled(20, 0)),
        throwsFormatException,
      );
      expect(
        () => architectureFromElfHeader(elfHeader(3)),
        throwsFormatException,
      );
    });
  });

  group('patch artifact URL', () {
    test('adds the artifact route to a canonical relative path', () {
      expect(
        buildPatchDownloadUri(
          baseUrl: 'http://192.168.99.109:8080',
          artifactPath: 'patches/delta.patch',
        ).toString(),
        'http://192.168.99.109:8080/artifacts/patches/delta.patch',
      );
    });

    test('does not duplicate an existing artifact route', () {
      expect(
        buildPatchDownloadUri(
          baseUrl: 'http://192.168.99.109:8080',
          artifactPath: '/artifacts/patches/delta.patch',
        ).toString(),
        'http://192.168.99.109:8080/artifacts/patches/delta.patch',
      );
    });

    test('rejects an artifact URL hosted elsewhere', () {
      expect(
        () => buildPatchDownloadUri(
          baseUrl: 'http://192.168.99.109:8080',
          artifactPath: 'https://example.com/delta.patch',
        ),
        throwsFormatException,
      );
    });
  });

  test('parses delta metadata from a patch manifest', () {
    final manifest = PatchManifest.fromJson({
      'manifest_version': 1,
      'key_id': 'test-key',
      'app_id': 'app-id',
      'release_id': 'release-id',
      'version': '1.0.0',
      'platform': 'android',
      'patch_number': 8,
      'artifact_path': 'patches/libapp-v1-to-v2.patch',
      'sha256': 'patch-hash',
      'file_size': 3083,
      'patch_format': 'bsdiff',
      'architecture': 'arm64-v8a',
      'base_sha256': 'base-hash',
      'base_patch_number': 7,
      'target_sha256': 'target-hash',
      'target_file_size': 3081136,
      'signature': 'signature',
    });

    expect(manifest.patchFormat, 'bsdiff');
    expect(manifest.architecture, 'arm64-v8a');
    expect(manifest.baseSha256, 'base-hash');
    expect(manifest.basePatchNumber, 7);
    expect(manifest.targetSha256, 'target-hash');
    expect(manifest.targetFileSize, 3081136);
  });

  test('canonical payload includes delta metadata', () {
    const verifier = PatchSignatureVerifier(publicKeyBase64: 'unused');

    final payload = verifier.buildPayload(
      manifestVersion: 1,
      keyId: 'test-key',
      appId: 'app-id',
      releaseId: 'release-id',
      version: '1.0.0',
      platform: 'android',
      patchNumber: 8,
      artifactPath: 'patches/libapp-v1-to-v2.patch',
      sha256: 'patch-hash',
      fileSize: 3083,
      patchFormat: 'bsdiff',
      architecture: 'arm64-v8a',
      baseSha256: 'base-hash',
      targetSha256: 'target-hash',
      targetFileSize: 3081136,
    );

    expect(
      payload,
      [
        'patchforge-manifest',
        '1',
        'test-key',
        'app-id',
        'release-id',
        '1.0.0',
        'android',
        '8',
        'patches/libapp-v1-to-v2.patch',
        'patch-hash',
        '3083',
        'bsdiff',
        'arm64-v8a',
        'base-hash',
        'target-hash',
        '3081136',
      ].join('\n'),
    );
  });

  test('manifest v2 signs channel, rollout, and parent patch', () {
    const verifier = PatchSignatureVerifier(publicKeyBase64: 'unused');
    final payload = verifier.buildPayload(
      manifestVersion: 2,
      keyId: 'test-key',
      appId: 'app-id',
      releaseId: 'release-id',
      version: '1.0.0',
      platform: 'android',
      patchNumber: 9,
      artifactPath: 'patches/v2.patch',
      sha256: 'patch-hash',
      fileSize: 2048,
      patchFormat: 'bsdiff',
      architecture: 'arm64-v8a',
      baseSha256: 'base-hash',
      basePatchNumber: 8,
      targetSha256: 'target-hash',
      targetFileSize: 4096,
      channel: 'beta',
      rolloutPercentage: 25,
    );

    expect(payload.endsWith('\n8\nbeta\n25'), isTrue);
  });

  test('verifies the published patch 8 manifest signature', () async {
    const verifier = PatchSignatureVerifier(
      publicKeyBase64: '6Xmg6UoF8mi5K8KeHPliblzkrG3jsRPI+siCE8HcJPw=',
    );

    final valid = await verifier.verify(
      manifestVersion: 1,
      keyId: 'main-2026',
      appId: '26460a95-4931-4fea-86ef-a6d8ef169476',
      releaseId: '300bb2a8-0a94-41d6-b80c-08f1d80f085c',
      version: '1.0.0',
      platform: 'android',
      patchNumber: 8,
      artifactPath: 'patches/libapp-v1-to-v2.patch',
      sha256:
          'f42e2b65678fa68f33c8b0751ce6fa2cf2ad8870cbd5be0bf296ab63fcad47d2',
      fileSize: 3083,
      patchFormat: 'bsdiff',
      architecture: 'arm64-v8a',
      baseSha256:
          'd8eddd99f161c14bd993de9dbcd09979d02ce9b98dadd62cd7b14834e18bc2ad',
      targetSha256:
          'd9ee1f12e93dc5483918efbf41fc6e00f55a3e071cd6bfb9d69f17caa36a3634',
      targetFileSize: 3081136,
      signatureBase64:
          'LnRWGwDAi6gR2ic0uZBgBu0+yBjXda8bxb6ZIoeCLYeagVRimUth5F84wYfasGdIHvi5OkQen5OO7M1pwQDuCQ==',
    );

    expect(valid, isTrue);
  });

  test('verifies the corrected runtime Patch 4 manifest signature', () async {
    const verifier = PatchSignatureVerifier(
      publicKeyBase64: '6Xmg6UoF8mi5K8KeHPliblzkrG3jsRPI+siCE8HcJPw=',
    );

    final valid = await verifier.verify(
      manifestVersion: 1,
      keyId: 'main-2026',
      appId: '03442aa7-7bec-4ca9-9f13-a095b49b7af4',
      releaseId: '56674d04-3199-49d1-b5c1-3909b5f64ead',
      version: '1.0.0',
      platform: 'android',
      patchNumber: 4,
      artifactPath:
          'patches/56674d04-3199-49d1-b5c1-3909b5f64ead-patch-4-1ad5f8614ed1.patch',
      sha256:
          '1ad5f8614ed156ffb2287e6a60b12bb7167fdc4df7297ae5d8e8014feb78a71b',
      fileSize: 6852,
      patchFormat: 'bsdiff',
      architecture: 'arm64-v8a',
      baseSha256:
          'f9246b223f03cfeece495687d2fa086696fd8eb4e70c3a4b81fdb4d013093aeb',
      targetSha256:
          '002c5ec6d9e69d13104ef6391aa26bb8137d26d06dd733dc9e740741a1a9aacb',
      targetFileSize: 3802032,
      signatureBase64:
          'qr+tZ6121f4xme6C/G3AQ91I2nXIUHsQT5kSbLoy4ZOepUc0QMKbl+IYJXu8+JFnHZYrYSbGO6+/iWJEH9WXDg==',
    );

    expect(valid, isTrue);
  });

  test('verifies the architecture-aware Patch 5 manifest signature', () async {
    const verifier = PatchSignatureVerifier(
      publicKeyBase64: '6Xmg6UoF8mi5K8KeHPliblzkrG3jsRPI+siCE8HcJPw=',
    );

    final valid = await verifier.verify(
      manifestVersion: 1,
      keyId: 'main-2026',
      appId: '03442aa7-7bec-4ca9-9f13-a095b49b7af4',
      releaseId: '56674d04-3199-49d1-b5c1-3909b5f64ead',
      version: '1.0.0',
      platform: 'android',
      patchNumber: 5,
      artifactPath:
          'patches/56674d04-3199-49d1-b5c1-3909b5f64ead-arm64-v8a-patch-5-92b0c7b05160.patch',
      sha256:
          '92b0c7b051603465c3af8ac355fa4faffcc5cc7242ad3ec81147e0f0bb003bce',
      fileSize: 360430,
      patchFormat: 'bsdiff',
      architecture: 'arm64-v8a',
      baseSha256:
          '002c5ec6d9e69d13104ef6391aa26bb8137d26d06dd733dc9e740741a1a9aacb',
      targetSha256:
          '4bb01efb5b033b69f945e0a83880991b001689bb57bafca2309838486306a56c',
      targetFileSize: 3802032,
      signatureBase64:
          'jnN84KQ07YYGNgguo1uD2CVotydPYMQcqTIdXZ/zzJIYSkHyJTdCB5J+8ZzEo5mYc1r2qL2BqwuOy4eJOqPpBQ==',
    );

    expect(valid, isTrue);
  });

  test('verifies the signed channel and rollout Patch 7 manifest', () async {
    const verifier = PatchSignatureVerifier(
      publicKeyBase64: '6Xmg6UoF8mi5K8KeHPliblzkrG3jsRPI+siCE8HcJPw=',
    );

    final valid = await verifier.verify(
      manifestVersion: 2,
      keyId: 'main-2026',
      appId: '03442aa7-7bec-4ca9-9f13-a095b49b7af4',
      releaseId: '56674d04-3199-49d1-b5c1-3909b5f64ead',
      version: '1.0.0',
      platform: 'android',
      patchNumber: 7,
      artifactPath:
          'patches/56674d04-3199-49d1-b5c1-3909b5f64ead-arm64-v8a-patch-7-cf7cba2a7cc1.patch',
      sha256:
          'cf7cba2a7cc19718d83847f1de3bfd0d5cc21f0102a98083008eb0314da2349d',
      fileSize: 8230,
      patchFormat: 'bsdiff',
      architecture: 'arm64-v8a',
      baseSha256:
          '3309446f0b1d82f12a2534851eed7d6845afabfd8056b68899b24761b0d6822b',
      basePatchNumber: 6,
      targetSha256:
          '1b57f021d4b2d03dbe440550914a7b781491ef6af6a453c20c5d6e68b91a4bdd',
      targetFileSize: 3802032,
      channel: 'stable',
      rolloutPercentage: 100,
      signatureBase64:
          'sJGL/vW2Y+2kxCsHhIhJEYHsYHy3nurp91H3FHDmSXrt4r9szKTD/ARvo52PxUha98qAGKsb2coUjbyBlYxVBQ==',
    );

    expect(valid, isTrue);
  });

  test('verifies the old-key-signed Patch 8 rotation bridge', () async {
    const verifier = PatchSignatureVerifier(
      publicKeyBase64: '6Xmg6UoF8mi5K8KeHPliblzkrG3jsRPI+siCE8HcJPw=',
    );
    final valid = await verifier.verify(
      manifestVersion: 2,
      keyId: 'main-2026',
      appId: '03442aa7-7bec-4ca9-9f13-a095b49b7af4',
      releaseId: '56674d04-3199-49d1-b5c1-3909b5f64ead',
      version: '1.0.0',
      platform: 'android',
      patchNumber: 8,
      artifactPath:
          'patches/56674d04-3199-49d1-b5c1-3909b5f64ead-arm64-v8a-patch-8-33ad3ce7f04f.patch',
      sha256:
          '33ad3ce7f04fcc1fa0a3473c4698ea11a9ad2f18b82c0935fe41db1a153d36e5',
      fileSize: 78654,
      patchFormat: 'bsdiff',
      architecture: 'arm64-v8a',
      baseSha256:
          '1b57f021d4b2d03dbe440550914a7b781491ef6af6a453c20c5d6e68b91a4bdd',
      basePatchNumber: 7,
      targetSha256:
          '204861b7c1bdcb46886305a6ee2937ab4c45a14fc876ad6d1e3002522048cd73',
      targetFileSize: 3802032,
      channel: 'stable',
      rolloutPercentage: 100,
      signatureBase64:
          'BR8+2Bg6K9ngSitG2fKK10UBcKwSwr5pr9k8hMWTItgHP8hz2TuRIcCzl8yycmO17N2M5dlYiUWil7jnwkCgDg==',
    );

    expect(valid, isTrue);
  });

  test('verifies Patch 9 with the rotated signing key', () async {
    const verifier = PatchSignatureVerifier(
      publicKeyBase64: 'ye/2ZzoUh3L6YHtXJACNJKQCwCl3ltaCF+qHtPBZt/U=',
    );
    final valid = await verifier.verify(
      manifestVersion: 2,
      keyId: 'rotation-2026',
      appId: '03442aa7-7bec-4ca9-9f13-a095b49b7af4',
      releaseId: '56674d04-3199-49d1-b5c1-3909b5f64ead',
      version: '1.0.0',
      platform: 'android',
      patchNumber: 9,
      artifactPath:
          'patches/56674d04-3199-49d1-b5c1-3909b5f64ead-arm64-v8a-patch-9-e20a30bcb646.patch',
      sha256:
          'e20a30bcb64619bdc823ff10bfcf46d671d0f97dead71c3dd26c4f6df4958967',
      fileSize: 17969,
      patchFormat: 'bsdiff',
      architecture: 'arm64-v8a',
      baseSha256:
          '204861b7c1bdcb46886305a6ee2937ab4c45a14fc876ad6d1e3002522048cd73',
      basePatchNumber: 8,
      targetSha256:
          '8bc03a6b2351358361c2473bf035c70b61b4945cd3da0cf189f7880055751ff1',
      targetFileSize: 3802032,
      channel: 'stable',
      rolloutPercentage: 100,
      signatureBase64:
          '8soD/Y/MfUH9Ch0+kI2ewepE/CPV6hbYhYjFowGwoFNvlwol76VwRnLEtFosXhNJJG+/XSeASi6PP8lHKm+RBg==',
    );

    expect(valid, isTrue);
  });

  test('applies a bsdiff-generated patch byte-for-byte', () {
    final base = base64Decode(
      'UGF0Y2hGb3JnZSBiYXNlbGluZSB2MQphYmNkZWYwMTIzNDU2Nzg5Cg==',
    );
    final patch = base64Decode(
      'QlNESUZGNDAvAAAAAAAAAC4AAAAAAAAAKAAAAAAAAABCWmg5MUFZJlNZF0NMfAAA'
      'DeAASUgEACAAIYTagwAMA25Eotb4u5IpwoSAuhpj4EJaaDkxQVkmU1mW17U0AA'
      'ABQADhIAQAIAAwzQCU9TDZDipBwu5IpwoSEtr2poBCWmg5MUFZJlNZpVyuAQAA'
      'BFGAABAgACKAFAAgACGmjNQhgGhUoeLuSKcKEhSrlcAg',
    );
    final expected = base64Decode(
      'UGF0Y2hGb3JnZSB0YXJnZXQgdjIKYWJjWFlaMDEyMzQ1Njc4OSEhCg==',
    );
    final reconstructed = BsdiffPatcher().apply(
      oldBytes: base,
      patchBytes: patch,
    );

    expect(reconstructed, orderedEquals(expected));
    expect(
      sha256.convert(reconstructed).toString(),
      'f0e04cbbd111f0133b6de3c30417087c6e482c2dbcf5fd00cbc68a8632e8d5ee',
    );
  });

  test('rejects a non-BSDIFF artifact', () {
    expect(
      () => BsdiffPatcher().apply(
        oldBytes: const [1, 2, 3],
        patchBytes: List<int>.filled(32, 0),
      ),
      throwsA(isA<BsdiffException>()),
    );
  });

  test('verifies the runtime activation patch manifest signature', () async {
    const verifier = PatchSignatureVerifier(
      publicKeyBase64: '6Xmg6UoF8mi5K8KeHPliblzkrG3jsRPI+siCE8HcJPw=',
    );

    final valid = await verifier.verify(
      manifestVersion: 1,
      keyId: 'main-2026',
      appId: '03442aa7-7bec-4ca9-9f13-a095b49b7af4',
      releaseId: '56674d04-3199-49d1-b5c1-3909b5f64ead',
      version: '1.0.0',
      platform: 'android',
      patchNumber: 1,
      artifactPath: 'patches/runtime-activation-v1.patch',
      sha256:
          '60ff7b5d1f1dc3008c42f00661d58f533317c5ca355e5ce33a72d595ec33a0e9',
      fileSize: 8078,
      patchFormat: 'bsdiff',
      architecture: 'arm64-v8a',
      baseSha256:
          '4968f0b94c3334a0c5a17c3bebf6542c5ffd19305a8a514421b9b42f19aef309',
      targetSha256:
          'bc3f3711a220b924a8123e7d1ae15bdd2a804d80c876888dd61fa4ea8a90dfa1',
      targetFileSize: 3802032,
      signatureBase64:
          'llDD6n2bWDJ5dnLJY3eBPjldWQ5SYalxuEUjyiD5XPVhTd+j0eBOB06QXOmf2815pD9eVZnE3Hr0WUcZVAJpCg==',
    );

    expect(valid, isTrue);
  });

  test('reconstructs a runtime-style target from a BSDIFF40 vector', () {
    final base = base64Decode(
      'cnVudGltZS1iYXNlbGluZS1BQUFBLUJCQkItQ0NDQy1ERERE',
    );
    final patch = base64Decode(
      'QlNESUZGNDArAAAAAAAAACUAAAAAAAAAJQAAAAAAAABCWmg5MUFZJlNZN/cl6wAA'
      'B+AASEAACiAAMM0AwaYMkx8XckU4UJA39yXrQlpoOTFBWSZTWZb7RKYAAABAAEQ'
      'AIAAhAIKDF3JFOFCQlvtEpkJaaDkxQVkmU1k0V1OcAAAOVYBAAAACPgEuQEQQIA'
      'AxTAATQr0mTRoPKPEbWA4mQ0hqaiikUa+LuSKcKEgaK6nOAA==',
    );
    final expected = base64Decode(
      'cnVudGltZS1wYXRjaGVkLUFBQUEtenotQkJCQi1DQ0NDLUVORA==',
    );
    final reconstructed = BsdiffPatcher().apply(
      oldBytes: base,
      patchBytes: patch,
    );

    expect(reconstructed, orderedEquals(expected));
    expect(
      sha256.convert(reconstructed).toString(),
      '9e0ca41af27ea812157e8e7ec8e3c48768c270121f4c2a1e09a2e872572aa10d',
    );
  });
}

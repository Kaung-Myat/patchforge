import 'dart:io';

import 'package:package_info_plus/package_info_plus.dart';

class AppMetadata {
  final String version;
  final String platform;

  const AppMetadata({required this.version, required this.platform});
}

class AppMetadataReader {
  Future<AppMetadata> read() async {
    final packageInfo = await PackageInfo.fromPlatform();

    String platform;

    if (Platform.isAndroid) {
      platform = 'android';
    } else if (Platform.isIOS) {
      platform = 'ios';
    } else {
      throw UnsupportedError(
        'PatchForge currently supports Android and iOS only.',
      );
    }

    return AppMetadata(version: packageInfo.version, platform: platform);
  }
}

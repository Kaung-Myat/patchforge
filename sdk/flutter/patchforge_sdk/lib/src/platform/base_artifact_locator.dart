import 'dart:io';
import 'dart:typed_data';

import 'package:flutter/services.dart';

String architectureFromElfHeader(List<int> header) {
  if (header.length < 20 ||
      header[0] != 0x7f ||
      header[1] != 0x45 ||
      header[2] != 0x4c ||
      header[3] != 0x46) {
    throw const FormatException('libapp.so does not have a valid ELF header.');
  }

  final endian = switch (header[5]) {
    1 => Endian.little,
    2 => Endian.big,
    _ => throw const FormatException('ELF byte order is not supported.'),
  };
  final machine = ByteData.sublistView(
    Uint8List.fromList(header),
  ).getUint16(18, endian);

  return switch (machine) {
    183 => 'arm64-v8a',
    40 => 'armeabi-v7a',
    62 => 'x86_64',
    _ => throw FormatException('Unsupported ELF machine type: $machine.'),
  };
}

class BaseArtifactLocator {
  static const _channel = MethodChannel('dev.patchforge/base_artifact');

  Future<File> locate(String architecture) async {
    if (!Platform.isAndroid) {
      throw UnsupportedError(
        'Automatic base libapp.so discovery currently supports Android only.',
      );
    }

    final path = await _channel.invokeMethod<String>('locateBaseArtifact', {
      'architecture': architecture,
    });

    if (path == null || path.isEmpty) {
      throw StateError('Android did not return a base libapp.so path.');
    }

    final file = File(path);

    if (!await file.exists()) {
      throw StateError('Base libapp.so does not exist at $path.');
    }

    return file;
  }

  Future<String> getArchitecture() async {
    if (!Platform.isAndroid) {
      throw UnsupportedError(
        'Automatic architecture discovery currently supports Android only.',
      );
    }

    try {
      final architecture = await _channel.invokeMethod<String>(
        'getArchitecture',
      );

      if (architecture != null &&
          const {'arm64-v8a', 'armeabi-v7a', 'x86_64'}.contains(architecture)) {
        return architecture;
      }
    } on MissingPluginException {
      // Installed apps using an older native plugin do not expose this method.
    }

    Object? lastError;

    for (final candidate in const ['arm64-v8a', 'armeabi-v7a', 'x86_64']) {
      try {
        final file = await locate(candidate);
        final handle = await file.open();

        try {
          return architectureFromElfHeader(await handle.read(20));
        } finally {
          await handle.close();
        }
      } catch (error) {
        lastError = error;
      }
    }

    throw StateError(
      'Could not determine the installed libapp.so architecture: $lastError',
    );
  }

  Future<int?> getLoadedPatch() async {
    if (!Platform.isAndroid) {
      return null;
    }

    return _channel.invokeMethod<int>('getLoadedPatch');
  }
}

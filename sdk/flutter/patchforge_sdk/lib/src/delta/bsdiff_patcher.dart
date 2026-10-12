import 'dart:typed_data';

import 'package:archive/archive.dart';

class BsdiffException implements Exception {
  final String message;

  const BsdiffException(this.message);

  @override
  String toString() => 'BsdiffException: $message';
}

class BsdiffPatcher {
  static const _headerSize = 32;
  static const _magic = 'BSDIFF40';

  Uint8List apply({
    required List<int> oldBytes,
    required List<int> patchBytes,
  }) {
    if (patchBytes.length < _headerSize) {
      throw const BsdiffException('Patch is shorter than the BSDIFF header.');
    }

    final patch = Uint8List.fromList(patchBytes);
    final magic = String.fromCharCodes(patch.sublist(0, 8));

    if (magic != _magic) {
      throw const BsdiffException('Unsupported patch header.');
    }

    final controlLength = _readSignedInt64(patch, 8);
    final diffLength = _readSignedInt64(patch, 16);
    final newSize = _readSignedInt64(patch, 24);

    if (controlLength < 0 || diffLength < 0 || newSize < 0) {
      throw const BsdiffException('Patch header contains a negative length.');
    }

    final controlEnd = _headerSize + controlLength;
    final diffEnd = controlEnd + diffLength;

    if (controlEnd > patch.length || diffEnd > patch.length) {
      throw const BsdiffException('Patch block lengths exceed the file size.');
    }

    try {
      final decoder = BZip2Decoder();
      final control = decoder.decodeBytes(
        patch.sublist(_headerSize, controlEnd),
        verify: true,
        maxOutputSize: (newSize * 3) + 24,
      );
      final diff = decoder.decodeBytes(
        patch.sublist(controlEnd, diffEnd),
        verify: true,
        maxOutputSize: newSize,
      );
      final extra = decoder.decodeBytes(
        patch.sublist(diffEnd),
        verify: true,
        maxOutputSize: newSize,
      );

      return _applyBlocks(
        oldBytes: oldBytes,
        newSize: newSize,
        control: control,
        diff: diff,
        extra: extra,
      );
    } catch (error) {
      if (error is BsdiffException) {
        rethrow;
      }

      throw BsdiffException('Failed to decompress patch blocks: $error');
    }
  }

  Uint8List _applyBlocks({
    required List<int> oldBytes,
    required int newSize,
    required Uint8List control,
    required Uint8List diff,
    required Uint8List extra,
  }) {
    final output = Uint8List(newSize);

    var controlPosition = 0;
    var diffPosition = 0;
    var extraPosition = 0;
    var oldPosition = 0;
    var newPosition = 0;

    while (newPosition < newSize) {
      if (controlPosition + 24 > control.length) {
        throw const BsdiffException('Control block ended unexpectedly.');
      }

      final addLength = _readSignedInt64(control, controlPosition);
      final copyLength = _readSignedInt64(control, controlPosition + 8);
      final seekLength = _readSignedInt64(control, controlPosition + 16);
      controlPosition += 24;

      if (addLength < 0 || copyLength < 0) {
        throw const BsdiffException(
          'Control block contains a negative length.',
        );
      }

      if (newPosition + addLength > newSize ||
          diffPosition + addLength > diff.length) {
        throw const BsdiffException('Diff block exceeds its declared bounds.');
      }

      for (var index = 0; index < addLength; index++) {
        final sourcePosition = oldPosition + index;
        final oldByte = sourcePosition >= 0 && sourcePosition < oldBytes.length
            ? oldBytes[sourcePosition]
            : 0;

        output[newPosition + index] =
            (diff[diffPosition + index] + oldByte) & 0xff;
      }

      newPosition += addLength;
      oldPosition += addLength;
      diffPosition += addLength;

      if (newPosition + copyLength > newSize ||
          extraPosition + copyLength > extra.length) {
        throw const BsdiffException('Extra block exceeds its declared bounds.');
      }

      output.setRange(
        newPosition,
        newPosition + copyLength,
        extra,
        extraPosition,
      );

      newPosition += copyLength;
      extraPosition += copyLength;
      oldPosition += seekLength;
    }

    return output;
  }

  int _readSignedInt64(Uint8List bytes, int offset) {
    if (offset < 0 || offset + 8 > bytes.length) {
      throw const BsdiffException('Cannot read a complete signed integer.');
    }

    var value = bytes[offset + 7] & 0x7f;

    for (var index = 6; index >= 0; index--) {
      value = (value * 256) + bytes[offset + index];
    }

    return (bytes[offset + 7] & 0x80) != 0 ? -value : value;
  }
}

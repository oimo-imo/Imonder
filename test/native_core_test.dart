import 'dart:ffi';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:imonder/core/native_core.dart';

/// Needs `cargo build --release` in rust/ first; skipped otherwise.
void main() {
  final path = 'rust/target/release/libimonder_core.so';
  final exists = File(path).existsSync();

  test('renders through the FFI boundary', () {
    final core = NativeCore(DynamicLibrary.open(File(path).absolute.path));
    core.orbit(0.2, 0.1);
    core.pan(0.01, 0.0);
    core.zoom(1.1);
    core.snapView(3);
    final px = core.render(160, 120, 1.0)!;
    expect(px.length, 160 * 120 * 4);
    expect(px[3], 255);
    expect(core.render(0, 10, 1.0), isNull);
    core.dispose();
  }, skip: exists ? false : 'rust library not built');

  test('edit mode: tap selects, handle drag moves, undo restores', () {
    final core = NativeCore(DynamicLibrary.open(File(path).absolute.path));
    core.snapView(3);
    core.snapAxis(3); // front
    core.setEditMode(true);
    core.setSelectMode(2);
    core.setMoveTool(true);
    core.render(400, 300, 1.0); // records the aspect ratio
    expect(core.status() & 4, 0);
    expect(core.tap(0.5, 0.5), isTrue);
    expect(core.status() & 4, 4);
    expect(core.tap(0.01, 0.01), isFalse);
    expect(core.status() & 4, 0);
    expect(core.undo(), isFalse);
    core.dispose();
  }, skip: exists ? false : 'rust library not built');
}

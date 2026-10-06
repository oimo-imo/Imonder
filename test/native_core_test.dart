import 'dart:ffi';
import 'dart:io';
import 'dart:typed_data';

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
    core.setTool(0);
    core.render(400, 300, 1.0); // records the aspect ratio
    expect(core.status() & 4, 0);
    expect(core.tap(0.5, 0.5), isTrue);
    expect(core.status() & 4, 4);
    expect(core.tap(0.01, 0.01), isFalse);
    expect(core.status() & 4, 0);
    expect(core.undo(), isFalse);
    core.dispose();
  }, skip: exists ? false : 'rust library not built');

  test('operations: extrude, adjust, range, undo', () {
    final core = NativeCore(DynamicLibrary.open(File(path).absolute.path));
    core.snapAxis(3);
    core.setEditMode(true);
    core.setSelectMode(2);
    core.render(400, 300, 1.0);
    expect(core.opBegin(0), isFalse, reason: 'nothing selected');
    core.tap(0.5, 0.5);
    expect(core.opBegin(0), isTrue);
    expect(core.status() & 8, 8);
    final r = core.opRange()!;
    expect((r.$1, r.$2, r.$3, r.$4), (-2.0, 2.0, 0.5, false));
    expect(core.opAdjust(1.0), isTrue);
    expect(core.opRange()!.$3, 1.0);
    expect(core.undo(), isTrue);
    expect(core.status() & 8, 0);
    expect(core.opRange(), isNull);
    core.setTool(3);
    core.dispose();
  }, skip: exists ? false : 'rust library not built');

  test('objects: add, list, hide, save and load', () {
    final core = NativeCore(DynamicLibrary.open(File(path).absolute.path));
    expect(core.objects().map((o) => o.name), ['立方体']);
    expect(core.addObject(4), isTrue);
    expect(core.addObject(99), isFalse);
    var list = core.objects();
    expect(list.map((o) => o.name), ['立方体', '球']);
    expect(list.last.active, isTrue);
    expect(core.setVisible(0, false), isTrue);
    expect(core.objects().first.visible, isFalse);
    expect(core.selectObject(0), isTrue);
    expect(core.selectObject(7), isFalse);

    final rev = core.revision();
    final bytes = core.save();
    expect(bytes.sublist(0, 4), [0x49, 0x4d, 0x4e, 0x44]); // "IMND"

    final other = NativeCore(DynamicLibrary.open(File(path).absolute.path));
    expect(other.load(bytes), isTrue);
    list = other.objects();
    expect(list.map((o) => o.name), ['立方体', '球']);
    expect(list.first.visible, isFalse);
    expect(other.load(Uint8List.fromList([1, 2, 3])), isFalse);
    expect(other.objects().length, 2, reason: 'a bad file changes nothing');
    other.newWork();
    expect(other.objects().map((o) => o.name), ['立方体']);

    expect(core.revision(), rev);
    expect(core.duplicateObject(), isTrue);
    expect(core.revision(), greaterThan(rev));
    expect(core.deleteObject(), isTrue);

    final thumb = core.thumbnail(64, 64)!;
    expect(thumb.length, 64 * 64 * 4);
    expect(core.thumbnail(0, 64), isNull);
    core.dispose();
    other.dispose();
  }, skip: exists ? false : 'rust library not built');
}

import 'dart:ffi';
import 'dart:io';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';

typedef _Create = Pointer<Void> Function();
typedef _Destroy = void Function(Pointer<Void>);
typedef _Vec2 = void Function(Pointer<Void>, double, double);
typedef _Vec1 = void Function(Pointer<Void>, double);
typedef _Int1 = void Function(Pointer<Void>, int);
typedef _Render = int Function(Pointer<Void>, int, int, double, Pointer<Uint8>);

DynamicLibrary _open() {
  if (Platform.isAndroid) return DynamicLibrary.open('libimonder_core.so');
  if (Platform.isWindows) return DynamicLibrary.open('imonder_core.dll');
  if (Platform.isMacOS) return DynamicLibrary.open('libimonder_core.dylib');
  return DynamicLibrary.open('libimonder_core.so');
}

/// Thin Dart wrapper over the Rust core's C ABI (see rust/src/lib.rs).
class NativeCore {
  NativeCore([DynamicLibrary? lib]) {
    final l = lib ?? _open();
    _destroy = l.lookupFunction<Void Function(Pointer<Void>), _Destroy>('imonder_destroy');
    _orbit = l.lookupFunction<Void Function(Pointer<Void>, Float, Float), _Vec2>('imonder_orbit');
    _pan = l.lookupFunction<Void Function(Pointer<Void>, Float, Float), _Vec2>('imonder_pan');
    _zoom = l.lookupFunction<Void Function(Pointer<Void>, Float), _Vec1>('imonder_zoom');
    _snap = l.lookupFunction<Void Function(Pointer<Void>, Int32), _Int1>('imonder_snap_view');
    _render = l.lookupFunction<
        Int32 Function(Pointer<Void>, Uint32, Uint32, Float, Pointer<Uint8>),
        _Render>('imonder_render');
    _handle = l.lookupFunction<_Create, _Create>('imonder_create')();
  }

  late final Pointer<Void> _handle;
  late final _Destroy _destroy;
  late final _Vec2 _orbit;
  late final _Vec2 _pan;
  late final _Vec1 _zoom;
  late final _Int1 _snap;
  late final _Render _render;

  Pointer<Uint8>? _buf;
  int _bufLen = 0;
  bool _disposed = false;

  /// Rotates the view; deltas are in radians.
  void orbit(double dx, double dy) => _orbit(_handle, dx, dy);

  /// Pans the view; deltas are fractions of the viewport height.
  void pan(double dx, double dy) => _pan(_handle, dx, dy);

  /// `factor` > 1 zooms in.
  void zoom(double factor) => _zoom(_handle, factor);

  /// 0 front, 1 right, 2 top, 3 perspective.
  void snapView(int preset) => _snap(_handle, preset);

  /// Renders an RGBA8 frame. The returned bytes are a fresh copy.
  Uint8List? render(int w, int h, double scale) {
    if (_disposed || w <= 0 || h <= 0) return null;
    final need = w * h * 4;
    if (_bufLen < need) {
      if (_buf != null) malloc.free(_buf!);
      _buf = malloc<Uint8>(need);
      _bufLen = need;
    }
    if (_render(_handle, w, h, scale, _buf!) != 1) return null;
    return Uint8List.fromList(_buf!.asTypedList(need));
  }

  void dispose() {
    if (_disposed) return;
    _disposed = true;
    _destroy(_handle);
    if (_buf != null) malloc.free(_buf!);
    _buf = null;
  }
}

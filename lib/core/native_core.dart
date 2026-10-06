import 'dart:ffi';
import 'dart:io';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';

typedef _Create = Pointer<Void> Function();
typedef _Destroy = void Function(Pointer<Void>);
typedef _Vec2 = void Function(Pointer<Void>, double, double);
typedef _Vec1 = void Function(Pointer<Void>, double);
typedef _Int1 = void Function(Pointer<Void>, int);
typedef _Angles = void Function(Pointer<Void>, Pointer<Float>);
typedef _Flag = void Function(Pointer<Void>, int);
typedef _Tap = int Function(Pointer<Void>, double, double, int);
typedef _Pt = int Function(Pointer<Void>, double, double);
typedef _Query = int Function(Pointer<Void>);
typedef _Op = int Function(Pointer<Void>, int);
typedef _OpAdjust = int Function(Pointer<Void>, double);
typedef _OpRange = int Function(Pointer<Void>, Pointer<Float>);
typedef _Void = void Function(Pointer<Void>);
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
    _snapAxis = l.lookupFunction<Void Function(Pointer<Void>, Int32), _Int1>('imonder_snap_axis');
    _angles = l.lookupFunction<Void Function(Pointer<Void>, Pointer<Float>), _Angles>('imonder_get_angles');
    _setEditMode = l.lookupFunction<Void Function(Pointer<Void>, Int32), _Flag>('imonder_set_edit_mode');
    _setSelectMode = l.lookupFunction<Void Function(Pointer<Void>, Int32), _Flag>('imonder_set_select_mode');
    _setTool = l.lookupFunction<Void Function(Pointer<Void>, Int32), _Flag>('imonder_set_tool');
    _selectAll = l.lookupFunction<Void Function(Pointer<Void>), _Void>('imonder_select_all');
    _opBegin = l.lookupFunction<Int32 Function(Pointer<Void>, Int32), _Op>('imonder_op_begin');
    _opAdjust = l.lookupFunction<Int32 Function(Pointer<Void>, Float), _OpAdjust>('imonder_op_adjust');
    _opRange = l.lookupFunction<Int32 Function(Pointer<Void>, Pointer<Float>), _OpRange>('imonder_op_range');
    _opCommit = l.lookupFunction<Void Function(Pointer<Void>), _Void>('imonder_op_commit');
    _tap = l.lookupFunction<Int32 Function(Pointer<Void>, Float, Float, Int32), _Tap>('imonder_tap');
    _dragBegin = l.lookupFunction<Int32 Function(Pointer<Void>, Float, Float), _Pt>('imonder_drag_begin');
    _dragUpdate = l.lookupFunction<Void Function(Pointer<Void>, Float, Float), _Vec2>('imonder_drag_update');
    _dragEnd = l.lookupFunction<Void Function(Pointer<Void>), _Destroy>('imonder_drag_end');
    _undo = l.lookupFunction<Int32 Function(Pointer<Void>), _Query>('imonder_undo');
    _redo = l.lookupFunction<Int32 Function(Pointer<Void>), _Query>('imonder_redo');
    _status = l.lookupFunction<Int32 Function(Pointer<Void>), _Query>('imonder_status');
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
  late final _Int1 _snapAxis;
  late final _Angles _angles;
  late final _Flag _setEditMode;
  late final _Flag _setSelectMode;
  late final _Flag _setTool;
  late final _Void _selectAll;
  late final _Op _opBegin;
  late final _OpAdjust _opAdjust;
  late final _OpRange _opRange;
  late final _Void _opCommit;
  late final _Tap _tap;
  late final _Pt _dragBegin;
  late final _Vec2 _dragUpdate;
  late final _Destroy _dragEnd;
  late final _Query _undo;
  late final _Query _redo;
  late final _Query _status;
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

  /// Looks along a world axis: 0 +X, 1 -X, 2 +Y, 3 -Y, 4 +Z, 5 -Z.
  void snapAxis(int axis) => _snapAxis(_handle, axis);

  /// Camera (yaw, pitch) in radians.
  (double, double) angles() {
    final p = malloc<Float>(2);
    try {
      _angles(_handle, p);
      return (p[0], p[1]);
    } finally {
      malloc.free(p);
    }
  }

  void setEditMode(bool on) => _setEditMode(_handle, on ? 1 : 0);

  /// 0 vertex, 1 edge, 2 face.
  void setSelectMode(int mode) => _setSelectMode(_handle, mode);

  /// 0 move, 1 rotate, 2 scale, 3 loop cut, anything else: none.
  void setTool(int tool) => _setTool(_handle, tool);

  void selectAll() => _selectAll(_handle);

  /// Runs an operation on the selection: 0 extrude, 1 inset, 2 loop cut, 3 bevel, 4 merge, 5 delete.
  /// False if it does not apply to the current selection.
  bool opBegin(int kind) => _opBegin(_handle, kind) == 1;

  /// Sets the value of the active operation (recomputes it from its base).
  bool opAdjust(double value) => _opAdjust(_handle, value) == 1;

  /// (min, max, current, isInteger) of the active operation, or null.
  (double, double, double, bool)? opRange() {
    final p = malloc<Float>(4);
    try {
      if (_opRange(_handle, p) != 1) return null;
      return (p[0], p[1], p[2], p[3] != 0);
    } finally {
      malloc.free(p);
    }
  }

  void opCommit() => _opCommit(_handle);

  /// Tap-select; `nx`/`ny` are fractions of the viewport. True if an element was hit.
  bool tap(double nx, double ny, {bool add = false}) => _tap(_handle, nx, ny, add ? 1 : 0) == 1;

  /// True if a move handle was grabbed.
  bool dragBegin(double nx, double ny) => _dragBegin(_handle, nx, ny) == 1;
  void dragUpdate(double dnx, double dny) => _dragUpdate(_handle, dnx, dny);
  void dragEnd() => _dragEnd(_handle);

  bool undo() => _undo(_handle) == 1;
  bool redo() => _redo(_handle) == 1;

  /// Bit 0 can undo, 1 can redo, 2 has selection, 3 adjustable operation active, 4-5 select mode.
  int status() => _status(_handle);

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

import 'dart:async';
import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../core/native_core.dart';

/// Longest side (in physical pixels) of the software-rendered frame.
/// Larger screens are rendered smaller and scaled up.
const _maxRenderSide = 1100;

class ModelViewportController {
  _ViewportState? _state;

  /// Camera (yaw, pitch) in radians; the view gizmo listens to this.
  final ValueNotifier<(double, double)> angles = ValueNotifier((-0.7, 0.5));

  /// Core status bits: 1 can undo, 2 can redo, 4 has selection.
  final ValueNotifier<int> status = ValueNotifier(0);

  bool _editMode = false;
  int _selectMode = 2;
  bool _moveTool = false;

  void _attach(_ViewportState s) {
    _state = s;
    s._core.setEditMode(_editMode);
    s._core.setSelectMode(_selectMode);
    s._core.setMoveTool(_moveTool);
  }

  void snapView(int preset) => _state?._snap(preset);
  void snapAxis(int axis) => _state?._snapAxis(axis);
  void orbit(double dx, double dy) => _state?._orbit(dx, dy);

  void setEditMode(bool on) {
    _editMode = on;
    _state?._core.setEditMode(on);
    _state?._changed();
  }

  void setSelectMode(int mode) {
    _selectMode = mode;
    _state?._core.setSelectMode(mode);
    _state?._changed();
  }

  void setMoveTool(bool on) {
    _moveTool = on;
    _state?._core.setMoveTool(on);
    _state?._changed();
  }

  void undo() {
    _state?._core.undo();
    _state?._changed();
  }

  void redo() {
    _state?._core.redo();
    _state?._changed();
  }
}

/// 3D viewport: draws frames from the Rust core and maps fingers / mouse to the camera.
///
/// Touch: 1 finger orbit, 2 fingers pan + pinch zoom, tap selects (edit mode),
/// 2-finger tap undo, 3-finger tap redo, drag a move handle to move the selection.
/// Mouse: left/middle drag orbit, Shift+drag or right drag pan, wheel zoom,
/// click selects (Shift adds).
class ModelViewport extends StatefulWidget {
  const ModelViewport({super.key, this.controller});
  final ModelViewportController? controller;

  @override
  State<ModelViewport> createState() => _ViewportState();
}

class _ViewportState extends State<ModelViewport> {
  late final NativeCore _core = NativeCore();
  final Map<int, Offset> _pointers = {};
  ui.Image? _image;
  Size _size = Size.zero;
  double _dpr = 1;
  bool _rendering = false;
  bool _dirty = true;

  @override
  void initState() {
    super.initState();
    widget.controller?._attach(this);
  }

  @override
  void dispose() {
    widget.controller?._state = null;
    _image?.dispose();
    _core.dispose();
    super.dispose();
  }

  void _orbit(double dx, double dy) {
    _core.orbit(dx, dy);
    _requestFrame();
  }

  /// Something other than the camera changed (selection, mesh, mode).
  void _changed() {
    widget.controller?.status.value = _core.status();
    _requestFrame();
  }

  void _snap(int preset) {
    _core.snapView(preset);
    _requestFrame();
  }

  void _snapAxis(int axis) {
    _core.snapAxis(axis);
    _requestFrame();
  }

  void _requestFrame() {
    widget.controller?.angles.value = _core.angles();
    _dirty = true;
    if (_rendering || _size.isEmpty) return;
    scheduleMicrotask(_renderFrame);
  }

  Future<void> _renderFrame() async {
    if (_rendering || !_dirty || !mounted) return;
    _rendering = true;
    _dirty = false;
    final k = math.min(1.0, _maxRenderSide / (math.max(_size.width, _size.height) * _dpr));
    final w = math.max(1, (_size.width * _dpr * k).round());
    final h = math.max(1, (_size.height * _dpr * k).round());
    final pixels = _core.render(w, h, _dpr * k);
    if (pixels != null) {
      final c = Completer<ui.Image>();
      ui.decodeImageFromPixels(pixels, w, h, ui.PixelFormat.rgba8888, c.complete);
      final img = await c.future;
      if (!mounted) {
        img.dispose();
        return;
      }
      final old = _image;
      setState(() => _image = img);
      old?.dispose();
    }
    _rendering = false;
    if (_dirty) _requestFrame();
  }

  // ---------------------------------------------------------------- input --

  static const _tapSlop = 14.0; // logical px of total travel that still counts as a tap
  static const _tapTime = Duration(milliseconds: 300);

  int _maxPointers = 0;
  double _travel = 0;
  Duration _downAt = Duration.zero;
  bool _handleDrag = false;
  bool _primaryButton = true;
  Offset _lastUp = Offset.zero;

  Offset _norm(Offset p) => Offset(p.dx / math.max(_size.width, 1), p.dy / math.max(_size.height, 1));

  void _down(PointerDownEvent e) {
    if (_pointers.isEmpty) {
      _maxPointers = 0;
      _travel = 0;
      _downAt = e.timeStamp;
      _handleDrag = false;
      _primaryButton = e.kind != PointerDeviceKind.mouse || (e.buttons & kPrimaryMouseButton) != 0;
      if (_primaryButton) {
        final n = _norm(e.localPosition);
        _handleDrag = _core.dragBegin(n.dx, n.dy);
        if (_handleDrag) _changed();
      }
    }
    _pointers[e.pointer] = e.localPosition;
    _maxPointers = math.max(_maxPointers, _pointers.length);
  }

  void _up(PointerEvent e) {
    final pos = _pointers.remove(e.pointer);
    if (pos != null) _lastUp = pos;
    if (_pointers.isNotEmpty) return;
    final quick = e is PointerUpEvent && _travel < _tapSlop && e.timeStamp - _downAt < _tapTime;
    if (_handleDrag) {
      _core.dragEnd();
      _handleDrag = false;
      _changed();
    } else if (quick) {
      if (_maxPointers == 1 && _primaryButton) {
        final n = _norm(_lastUp);
        _core.tap(n.dx, n.dy, add: HardwareKeyboard.instance.isShiftPressed);
        _changed();
      } else if (_maxPointers == 2) {
        widget.controller?.undo();
      } else if (_maxPointers == 3) {
        widget.controller?.redo();
      }
    }
  }

  void _move(PointerMoveEvent e) {
    final prev = _pointers[e.pointer];
    if (prev == null) return;
    final h = math.max(_size.height, 1.0);
    _travel += (e.localPosition - prev).distance;

    if (_handleDrag) {
      final d = e.localPosition - prev;
      _core.dragUpdate(d.dx / math.max(_size.width, 1), d.dy / h);
      _pointers[e.pointer] = e.localPosition;
      _requestFrame();
      return;
    }

    if (e.kind == PointerDeviceKind.mouse) {
      final d = e.localPosition - prev;
      final shift = HardwareKeyboard.instance.isShiftPressed;
      final pan = (e.buttons & kSecondaryMouseButton) != 0 || shift;
      if (pan) {
        _core.pan(d.dx / h, d.dy / h);
      } else {
        _core.orbit(d.dx * 0.008, d.dy * 0.008);
      }
    } else if (_pointers.length == 1) {
      final d = e.localPosition - prev;
      _core.orbit(d.dx * 0.008, d.dy * 0.008);
    } else if (_pointers.length == 2) {
      final before = _pointers.values.toList();
      _pointers[e.pointer] = e.localPosition;
      final after = _pointers.values.toList();
      final c0 = (before[0] + before[1]) / 2, c1 = (after[0] + after[1]) / 2;
      final d0 = (before[0] - before[1]).distance, d1 = (after[0] - after[1]).distance;
      final dc = c1 - c0;
      _core.pan(dc.dx / h, dc.dy / h);
      if (d0 > 8 && d1 > 8) _core.zoom(d1 / d0);
    }
    _pointers[e.pointer] = e.localPosition;
    _requestFrame();
  }

  void _signal(PointerSignalEvent e) {
    if (e is PointerScrollEvent) {
      _core.zoom(math.exp(-e.scrollDelta.dy * 0.0015));
      _requestFrame();
    }
  }

  @override
  Widget build(BuildContext context) {
    _dpr = MediaQuery.devicePixelRatioOf(context);
    return LayoutBuilder(builder: (context, c) {
      final size = Size(c.maxWidth, c.maxHeight);
      if (size != _size) {
        _size = size;
        WidgetsBinding.instance.addPostFrameCallback((_) => _requestFrame());
      }
      return Listener(
        behavior: HitTestBehavior.opaque,
        onPointerDown: _down,
        onPointerMove: _move,
        onPointerUp: _up,
        onPointerCancel: _up,
        onPointerSignal: _signal,
        child: ColoredBox(
          color: const Color(0xFF1E1E21),
          child: _image == null
              ? const SizedBox.expand()
              : RawImage(
                  image: _image,
                  fit: BoxFit.fill,
                  filterQuality: FilterQuality.medium,
                  width: size.width,
                  height: size.height,
                ),
        ),
      );
    });
  }
}

import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:gal/gal.dart';

import 'viewport.dart';

const _panel = Color(0xFF252528);
const _muted = Color(0xFF8E8E95);

/// Width : height of the image.
const captureAspects = <(String, double)>[('1:1', 1), ('4:5', 4 / 5), ('3:4', 3 / 4), ('16:9', 16 / 9), ('9:16', 9 / 16)];

/// Preset ids match `capture_preset` in rust/src/lib.rs.
const capturePresets = <(String, IconData)>[
  ('箱庭', Icons.view_in_ar),
  ('斜め', Icons.threed_rotation),
  ('正面', Icons.crop_portrait),
  ('真上', Icons.grid_on),
  ('横', Icons.crop_landscape),
];

const captureSizes = <int>[1024, 2048, 4096];

/// The largest rectangle of [aspect] (w/h) centred in [area].
Rect fitFrame(Rect area, double aspect) {
  var w = area.width, h = w / aspect;
  if (h > area.height) {
    h = area.height;
    w = h * aspect;
  }
  return Rect.fromCenter(center: area.center, width: w, height: h);
}

/// Capture mode: a frame over the 3D view, presets to place the camera, and saving to the
/// device's photo gallery. The view stays live, so the shot can be adjusted with the usual gestures.
class CaptureOverlay extends StatefulWidget {
  const CaptureOverlay({super.key, required this.viewport, required this.onClose, this.saveImage});
  final ModelViewportController viewport;
  final VoidCallback onClose;

  /// Where images go; defaults to the system photo gallery (album "Imonder").
  final Future<void> Function(Uint8List png, String name)? saveImage;

  @override
  State<CaptureOverlay> createState() => _CaptureOverlayState();
}

class _CaptureOverlayState extends State<CaptureOverlay> {
  int _aspect = 0;
  int _size = 1;
  int? _preset;
  bool _grid = false;
  bool _edges = true;
  bool _transparent = false;
  bool _busy = false;
  Rect _frame = Rect.zero;

  ModelViewportController get _vp => widget.viewport;
  bool get _ortho => _vp.status.value & 256 != 0;

  @override
  void initState() {
    super.initState();
    // Not during build: entering capture mode notifies listeners of the editor above us.
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) _vp.setCapture(true, grid: _grid, edges: _edges);
    });
  }

  void _close() {
    _vp.setCapture(false);
    widget.onClose();
  }

  void _options() => _vp.setCapture(true, grid: _grid, edges: _edges);

  void _toast(String text) {
    ScaffoldMessenger.of(context)
      ..hideCurrentSnackBar()
      ..showSnackBar(SnackBar(content: Text(text), duration: const Duration(seconds: 3)));
  }

  static Future<void> _toGallery(Uint8List png, String name) async {
    if (!await Gal.hasPermission(toAlbum: true) && !await Gal.requestPermission(toAlbum: true)) {
      throw StateError('写真への保存が許可されていません');
    }
    await Gal.putImageBytes(png, album: 'Imonder', name: name);
  }

  Future<void> _save() async {
    setState(() => _busy = true);
    try {
      final png = await _vp.exportPng(_frame, captureSizes[_size], transparent: _transparent);
      if (png == null) throw StateError('画像を作れませんでした');
      final now = DateTime.now();
      String two(int n) => n.toString().padLeft(2, '0');
      final name = 'imonder_${now.year}${two(now.month)}${two(now.day)}_${two(now.hour)}${two(now.minute)}${two(now.second)}';
      await (widget.saveImage ?? _toGallery)(png, name);
      _toast('写真に保存しました（$name.png）');
    } catch (e) {
      _toast('保存できませんでした: $e');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final pad = MediaQuery.paddingOf(context);
    final accent = Theme.of(context).colorScheme.primary;
    return LayoutBuilder(builder: (context, c) {
      const panelHeight = 196.0;
      final area = Rect.fromLTRB(16, pad.top + 52 + 16, c.maxWidth - 16, c.maxHeight - panelHeight - pad.bottom - 16);
      _frame = area.isEmpty ? Rect.zero : fitFrame(area, captureAspects[_aspect].$2);
      return Stack(children: [
        Positioned.fill(child: IgnorePointer(child: CustomPaint(painter: _FramePainter(_frame)))),
        Positioned(top: 0, left: 0, right: 0, child: _topBar(pad, accent)),
        Positioned(left: 0, right: 0, bottom: 0, child: _panelWidget(pad, accent, panelHeight)),
      ]);
    });
  }

  Widget _topBar(EdgeInsets pad, Color accent) {
    return Container(
      color: const Color(0xFF161618),
      padding: EdgeInsets.fromLTRB(6, pad.top, 10, 0),
      height: 52 + pad.top,
      child: Row(children: [
        IconButton(tooltip: '撮影をやめる', onPressed: _busy ? null : _close, icon: const Icon(Icons.close)),
        const Text('撮影', style: TextStyle(fontSize: 16, fontWeight: FontWeight.w600)),
        const Spacer(),
        FilledButton.icon(
          onPressed: _busy || _frame.isEmpty ? null : _save,
          icon: _busy
              ? const SizedBox(width: 16, height: 16, child: CircularProgressIndicator(strokeWidth: 2))
              : const Icon(Icons.download, size: 18),
          label: Text('保存 ${captureSizes[_size] ~/ 1024}K'),
        ),
      ]),
    );
  }

  Widget _chip(String label, bool selected, VoidCallback onTap, {IconData? icon}) {
    return Padding(
      padding: const EdgeInsets.only(right: 8),
      child: ChoiceChip(
        avatar: icon == null ? null : Icon(icon, size: 16),
        label: Text(label),
        selected: selected,
        showCheckmark: false,
        onSelected: (_) => onTap(),
        materialTapTargetSize: MaterialTapTargetSize.padded,
      ),
    );
  }

  Widget _row(List<Widget> children) =>
      SizedBox(height: 48, child: ListView(scrollDirection: Axis.horizontal, padding: const EdgeInsets.symmetric(horizontal: 12), children: children));

  Widget _panelWidget(EdgeInsets pad, Color accent, double height) {
    return Container(
      height: height + pad.bottom,
      padding: EdgeInsets.only(top: 10, bottom: pad.bottom + 8),
      decoration: const BoxDecoration(color: _panel, borderRadius: BorderRadius.vertical(top: Radius.circular(22))),
      child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
        _row([
          for (var i = 0; i < capturePresets.length; i++)
            _chip(capturePresets[i].$1, _preset == i, () {
              setState(() => _preset = i);
              _vp.applyPreset(i, _frame);
            }, icon: capturePresets[i].$2),
          _chip('全体を収める', false, () => _vp.frameAll(_frame), icon: Icons.fit_screen),
        ]),
        _row([
          for (var i = 0; i < captureAspects.length; i++)
            _chip(captureAspects[i].$1, _aspect == i, () => setState(() => _aspect = i)),
          const SizedBox(width: 8),
          for (var i = 0; i < captureSizes.length; i++)
            _chip('${captureSizes[i] ~/ 1024}K', _size == i, () => setState(() => _size = i)),
        ]),
        _row([
          _chip('平行投影', _ortho, () {
            _vp.setOrtho(!_ortho);
            setState(() {});
          }),
          _chip('グリッド', _grid, () {
            setState(() => _grid = !_grid);
            _options();
          }),
          _chip('辺', _edges, () {
            setState(() => _edges = !_edges);
            _options();
          }),
          _chip('背景を透明に', _transparent, () => setState(() => _transparent = !_transparent)),
        ]),
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 2, 16, 0),
          child: Text(
            _transparent ? '背景は透明で保存されます。指で回転・移動・ズームして調整できます' : '指で回転・移動・ズームして調整できます',
            style: const TextStyle(fontSize: 12, color: _muted),
          ),
        ),
      ]),
    );
  }
}

/// Dims everything outside the frame and outlines it.
class _FramePainter extends CustomPainter {
  _FramePainter(this.frame);
  final Rect frame;

  @override
  void paint(Canvas canvas, Size size) {
    if (frame.isEmpty) return;
    final outside = Path()
      ..fillType = PathFillType.evenOdd
      ..addRect(Offset.zero & size)
      ..addRect(frame);
    canvas.drawPath(outside, Paint()..color = const Color(0x99000000));
    canvas.drawRect(
      frame,
      Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = 1.2
        ..color = const Color(0xCCFFFFFF),
    );
    // Rule-of-thirds guides, faint.
    final guide = Paint()
      ..color = const Color(0x33FFFFFF)
      ..strokeWidth = 1;
    for (var i = 1; i < 3; i++) {
      final x = frame.left + frame.width * i / 3, y = frame.top + frame.height * i / 3;
      canvas.drawLine(Offset(x, frame.top), Offset(x, frame.bottom), guide);
      canvas.drawLine(Offset(frame.left, y), Offset(frame.right, y), guide);
    }
  }

  @override
  bool shouldRepaint(_FramePainter old) => old.frame != frame;
}

import 'dart:math' as math;

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';

const _axisColors = [Color(0xFFE5534B), Color(0xFF7BC86C), Color(0xFF5B8DEF)]; // X Y Z
const _axisNames = ['X', 'Y', 'Z'];

/// One end of an axis, projected onto the screen.
class GizmoBubble {
  GizmoBubble(this.axis, this.positive, this.pos, this.depth);
  final int axis; // 0 X, 1 Y, 2 Z
  final bool positive;
  final Offset pos; // relative to the gizmo centre, y down
  final double depth; // > 0 = towards the viewer
  int get snapId => axis * 2 + (positive ? 0 : 1); // matches camera.snap_axis
}

/// Blender-style navigation gizmo. Tap an axis to look along it; tap the
/// background to go back to the default perspective view.
class ViewGizmo extends StatelessWidget {
  const ViewGizmo({
    super.key,
    required this.angles,
    required this.onAxis,
    required this.onReset,
    required this.onOrbit,
    this.size = 112,
  });

  final ValueListenable<(double, double)> angles;
  final ValueChanged<int> onAxis;
  final VoidCallback onReset;

  /// Dragging the gizmo rotates the view (radians).
  final void Function(double dx, double dy) onOrbit;
  final double size;

  /// Projects the six axis ends for the given camera yaw / pitch (Z-up).
  static List<GizmoBubble> project(double yaw, double pitch, double radius) {
    final eye = _V(math.cos(yaw) * math.cos(pitch), math.sin(yaw) * math.cos(pitch), math.sin(pitch));
    final f = eye.scale(-1);
    var right = f.cross(const _V(0, 0, 1)).norm();
    if (right.len < 1e-6) right = const _V(1, 0, 0);
    final up = right.cross(f);
    final out = <GizmoBubble>[];
    for (var axis = 0; axis < 3; axis++) {
      for (final positive in [true, false]) {
        final sign = positive ? 1.0 : -1.0;
        final a = _V(axis == 0 ? sign : 0, axis == 1 ? sign : 0, axis == 2 ? sign : 0);
        out.add(GizmoBubble(axis, positive, Offset(a.dot(right), -a.dot(up)) * radius, a.dot(eye)));
      }
    }
    out.sort((a, b) => a.depth.compareTo(b.depth)); // far first, so near ones paint on top
    return out;
  }

  static const double _bubbleR = 13;

  @override
  Widget build(BuildContext context) {
    final r = size / 2 - _bubbleR - 2;
    return SizedBox(
      width: size,
      height: size,
      child: ValueListenableBuilder<(double, double)>(
        valueListenable: angles,
        builder: (context, a, _) {
          final bubbles = project(a.$1, a.$2, r);
          return Semantics(
            label: '視点ギズモ',
            child: GestureDetector(
              behavior: HitTestBehavior.opaque,
              onPanUpdate: (d) => onOrbit(d.delta.dx * 0.012, d.delta.dy * 0.012),
              onTapUp: (d) {
                final p = d.localPosition - Offset(size / 2, size / 2);
                GizmoBubble? best;
                for (final b in bubbles.reversed) {
                  // reversed = nearest first
                  if ((b.pos - p).distance <= _bubbleR + 8) {
                    best = b;
                    break;
                  }
                }
                if (best != null) {
                  onAxis(best.snapId);
                } else {
                  onReset();
                }
              },
              child: CustomPaint(painter: _GizmoPainter(bubbles)),
            ),
          );
        },
      ),
    );
  }
}

class _GizmoPainter extends CustomPainter {
  _GizmoPainter(this.bubbles);
  final List<GizmoBubble> bubbles;

  @override
  void paint(Canvas canvas, Size size) {
    final c = size.center(Offset.zero);
    canvas.drawCircle(c, size.width / 2, Paint()..color = const Color(0x552A2A2E));
    for (final b in bubbles) {
      final col = _axisColors[b.axis];
      final facing = b.depth > -0.2 ? 1.0 : 0.55; // dim the ones pointing away
      final p = c + b.pos;
      if (b.positive) {
        canvas.drawLine(c, p, Paint()..color = col.withValues(alpha: 0.8 * facing)..strokeWidth = 2);
        canvas.drawCircle(p, ViewGizmo._bubbleR, Paint()..color = col.withValues(alpha: facing));
        final tp = TextPainter(
          text: TextSpan(
            text: _axisNames[b.axis],
            style: const TextStyle(color: Color(0xFF1A1A1C), fontSize: 13, fontWeight: FontWeight.w700),
          ),
          textDirection: TextDirection.ltr,
        )..layout();
        tp.paint(canvas, p - Offset(tp.width / 2, tp.height / 2));
      } else {
        canvas.drawCircle(p, ViewGizmo._bubbleR - 3, Paint()..color = col.withValues(alpha: 0.35 * facing));
        canvas.drawCircle(
            p,
            ViewGizmo._bubbleR - 3,
            Paint()
              ..style = PaintingStyle.stroke
              ..strokeWidth = 1.5
              ..color = col.withValues(alpha: 0.9 * facing));
      }
    }
  }

  @override
  bool shouldRepaint(_GizmoPainter old) => true;
}

class _V {
  const _V(this.x, this.y, this.z);
  final double x, y, z;
  _V scale(double s) => _V(x * s, y * s, z * s);
  double dot(_V o) => x * o.x + y * o.y + z * o.z;
  _V cross(_V o) => _V(y * o.z - z * o.y, z * o.x - x * o.z, x * o.y - y * o.x);
  double get len => math.sqrt(dot(this));
  _V norm() => len < 1e-9 ? this : scale(1 / len);
}

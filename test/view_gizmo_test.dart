import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:imonder/ui/view_gizmo.dart';

void main() {
  test('looking along -Y (front): +X is on the right, +Z up, -Y nearest', () {
    final b = ViewGizmo.project(-math.pi / 2, 0, 40);
    final x = b.firstWhere((e) => e.axis == 0 && e.positive);
    final z = b.firstWhere((e) => e.axis == 2 && e.positive);
    expect(x.pos.dx, closeTo(40, 0.01));
    expect(z.pos.dy, closeTo(-40, 0.01));
    expect(b.last.axis, 1);
    expect(b.last.positive, isFalse);
    expect(b.last.snapId, 3);
  });

  testWidgets('tapping the +X bubble reports axis 0, background resets', (t) async {
    final angles = ValueNotifier<(double, double)>((-math.pi / 2, 0.0));
    int? axis;
    var reset = false;
    await t.pumpWidget(MaterialApp(
      home: Center(
        child: ViewGizmo(angles: angles, onAxis: (a) => axis = a, onReset: () => reset = true, onOrbit: (_, _) {}),
      ),
    ));
    final centre = t.getCenter(find.byType(ViewGizmo));
    await t.tapAt(centre + const Offset(40, 0)); // +X bubble
    expect(axis, 0);
    await t.tapAt(centre + const Offset(-38, 38)); // empty corner
    expect(reset, isTrue);
  });
}

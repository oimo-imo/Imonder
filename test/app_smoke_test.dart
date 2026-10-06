import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:imonder/core/native_core.dart' as core;
import 'package:imonder/storage/work_store.dart';
import 'package:imonder/ui/editor_screen.dart';
import 'package:imonder/ui/gallery_screen.dart';

/// Real file IO finishes outside the fake-async zone of widget tests, and every `await` in a
/// chain needs another pump before it continues, so alternate real waiting and pumping.
Future<void> settle(WidgetTester tester, {int rounds = 120}) async {
  for (var i = 0; i < rounds; i++) {
    await tester.runAsync(() => Future<void>.delayed(const Duration(milliseconds: 10)));
    await tester.pump(const Duration(milliseconds: 10));
  }
}

/// Runs the real UI against the real Rust library. Needs `cargo build --release` in rust/ first.
void main() {
  final lib = File('rust/target/release/libimonder_core.so');
  final skip = !lib.existsSync();
  late Directory docs;

  setUp(() {
    core.debugLibraryPath = lib.absolute.path;
    docs = Directory.systemTemp.createTempSync('imonder_app_');
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger.setMockMethodCallHandler(
      const MethodChannel('plugins.flutter.io/path_provider'),
      (call) async => docs.path,
    );
  });

  tearDown(() {
    core.debugLibraryPath = null;
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(const MethodChannel('plugins.flutter.io/path_provider'), null);
    docs.deleteSync(recursive: true);
  });

  testWidgets('the editor starts a work, saves it and offers object tools', (tester) async {
    tester.view.physicalSize = const Size(780, 1688);
    tester.view.devicePixelRatio = 2.0;
    addTearDown(tester.view.reset);

    await tester.pumpWidget(const MaterialApp(home: EditorScreen()));
    await settle(tester);
    final works = Directory('${docs.path}/works');
    expect(works.existsSync(), isTrue);
    expect(works.listSync().where((e) => e.path.endsWith('.imnd')), hasLength(1));
    expect(works.listSync().where((e) => e.path.endsWith('.png')), hasLength(1), reason: 'thumbnail written');

    expect(find.byTooltip('追加'), findsOneWidget);
    expect(find.byTooltip('ギャラリー'), findsOneWidget);
    await tester.tap(find.byTooltip('追加'));
    await tester.pumpAndSettle();
    expect(find.text('トーラス'), findsOneWidget);
    await tester.tap(find.text('球'));
    await tester.pumpAndSettle();

    // Objects are listed in the scene sheet.
    await tester.tap(find.byTooltip('シーン'));
    await tester.pumpAndSettle();
    expect(find.text('立方体'), findsOneWidget);
    expect(find.text('球'), findsOneWidget);

    // Close the scene sheet, leave the editor (saves on the way out) and start the app again.
    await tester.tapAt(const Offset(10, 10));
    await tester.pumpAndSettle();
    await tester.pumpWidget(const SizedBox()); // dispose: cancels the autosave timer
    await settle(tester, rounds: 40);

    await tester.pumpWidget(const MaterialApp(home: EditorScreen()));
    await settle(tester);
    await tester.tap(find.byTooltip('シーン'));
    await tester.pumpAndSettle();
    expect(find.text('立方体'), findsOneWidget, reason: 'the work came back after a restart');
    expect(find.text('球'), findsOneWidget);
    expect(works.listSync().where((e) => e.path.endsWith('.imnd')), hasLength(1), reason: 'reopened, not duplicated');

    await tester.tapAt(const Offset(10, 10));
    await tester.pumpAndSettle();
    await tester.pumpWidget(const SizedBox());
    await settle(tester, rounds: 30);
  }, skip: skip);

  testWidgets('the gallery lists works and reports what was chosen', (tester) async {
    final store = WorkStore(Directory('${docs.path}/w')..createSync());
    final a = await tester.runAsync(() => store.create(name: 'ねこ'));
    await tester.runAsync(() => store.create(name: 'いぬ'));
    GalleryChoice? choice;
    await tester.pumpWidget(MaterialApp(
      home: Builder(
        builder: (context) => TextButton(
          onPressed: () async => choice = await Navigator.of(context)
              .push<GalleryChoice>(MaterialPageRoute(builder: (_) => GalleryScreen(store: store, currentId: a))),
          child: const Text('open'),
        ),
      ),
    ));
    await tester.tap(find.text('open'));
    await settle(tester, rounds: 30);
    expect(find.text('ねこ'), findsOneWidget);
    expect(find.text('いぬ'), findsOneWidget);
    await tester.tap(find.text('ねこ'));
    await tester.pumpAndSettle();
    expect(choice, isA<OpenWork>());
    expect((choice as OpenWork).id, a);

    choice = null;
    await tester.tap(find.text('open'));
    await settle(tester, rounds: 30);
    await tester.tap(find.text('新規'));
    await tester.pumpAndSettle();
    expect(choice, isA<NewWork>());
  });
}

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

  testWidgets('capture: diorama preset, square 2K PNG saved, back to editing', (tester) async {
    tester.view.physicalSize = const Size(780, 1688);
    tester.view.devicePixelRatio = 2.0;
    addTearDown(tester.view.reset);
    final saved = <(Uint8List, String)>[];

    await tester.pumpWidget(MaterialApp(
      home: EditorScreen(saveCapture: (png, name) async => saved.add((png, name))),
    ));
    await settle(tester);
    await tester.tap(find.byTooltip('撮影'));
    await settle(tester, rounds: 20);
    expect(find.text('箱庭'), findsOneWidget);
    expect(find.byTooltip('ギャラリー'), findsNothing, reason: 'editing UI is hidden while capturing');
    await tester.tap(find.text('箱庭'));
    await settle(tester, rounds: 20);
    await tester.tap(find.text('保存 2K'));
    await settle(tester, rounds: 200);
    expect(saved, hasLength(1));
    final (png, name) = saved.single;
    expect(name, startsWith('imonder_'));
    expect(png.sublist(1, 4), 'PNG'.codeUnits);
    final data = ByteData.sublistView(png);
    expect((data.getUint32(16), data.getUint32(20)), (2048, 2048), reason: 'square, 2K');
    expect(find.textContaining('写真に保存しました'), findsOneWidget);

    await tester.tap(find.byTooltip('撮影をやめる'));
    await settle(tester, rounds: 20);
    expect(find.byTooltip('ギャラリー'), findsOneWidget);
    expect(find.text('透視'), findsOneWidget, reason: 'the editing view is perspective again');

    await tester.pumpWidget(const SizedBox());
    await settle(tester, rounds: 30);
  }, skip: skip);

  testWidgets('presets: save visible objects, insert them, start a new work from one; snap toggle', (tester) async {
    tester.view.physicalSize = const Size(780, 1688);
    tester.view.devicePixelRatio = 2.0;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(const MaterialApp(home: EditorScreen()));
    await settle(tester);

    Future<List<String>> sceneNames() async {
      await tester.tap(find.byTooltip('シーン'));
      await tester.pumpAndSettle();
      final names = tester
          .widgetList<ListTile>(find.byType(ListTile))
          .map((t) => (t.title as Text).data!)
          .toList();
      await tester.tapAt(const Offset(10, 10));
      await tester.pumpAndSettle();
      return names;
    }

    // Snap toggle.
    expect(find.byTooltip('スナップ：オフ'), findsOneWidget);
    await tester.tap(find.byTooltip('スナップ：オフ'));
    await tester.pump();
    expect(find.byTooltip('スナップ：オン'), findsOneWidget);

    // Cube + sphere, saved as a preset.
    await tester.tap(find.byTooltip('追加'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('球'));
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip('シーン'));
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip('プリセットに登録'));
    await settle(tester, rounds: 20);
    await tester.enterText(find.byType(TextField), 'へや');
    await tester.tap(find.text('登録'));
    await settle(tester, rounds: 60);
    expect(find.textContaining('「へや」をプリセットに登録しました'), findsOneWidget);

    // Inserted from the add menu: the objects come in next to the existing ones.
    await tester.tap(find.byTooltip('追加'));
    await settle(tester, rounds: 30);
    expect(find.text('へや'), findsOneWidget);
    await tester.tap(find.text('へや'));
    await settle(tester, rounds: 30);
    expect(await sceneNames(), ['立方体', '球', '立方体.001', '球.001']);

    // A new work started from the preset holds exactly its objects.
    await tester.tap(find.byTooltip('ギャラリー'));
    await settle(tester, rounds: 40);
    await tester.tap(find.text('新規'));
    await settle(tester, rounds: 30);
    expect(find.text('空の作品（立方体ひとつ）'), findsOneWidget);
    await tester.tap(find.text('へや'));
    await settle(tester, rounds: 60);
    expect(await sceneNames(), ['立方体', '球']);

    await tester.pumpWidget(const SizedBox());
    await settle(tester, rounds: 30);
  }, skip: skip);

  testWidgets('multi-select from the toolbar, grouping shown in the scene list', (tester) async {
    tester.view.physicalSize = const Size(780, 1688);
    tester.view.devicePixelRatio = 2.0;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(const MaterialApp(home: EditorScreen()));
    await settle(tester);
    await tester.tap(find.byTooltip('追加'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('球'));
    await settle(tester, rounds: 10);

    await tester.tap(find.byTooltip('グループ化'));
    await settle(tester, rounds: 10);
    expect(find.textContaining('2つ以上のオブジェクトを選んでください'), findsOneWidget);
    await tester.tap(find.byTooltip('全選択'));
    await settle(tester, rounds: 10);
    await tester.tap(find.byTooltip('グループ化'));
    await settle(tester, rounds: 10);
    expect(find.byTooltip('グループ解除'), findsOneWidget);

    await tester.tap(find.byTooltip('シーン'));
    await tester.pumpAndSettle();
    expect(find.text('グループ 1'), findsNWidgets(2));
    expect(tester.widgetList<Checkbox>(find.byType(Checkbox)).every((c) => c.value == true), isTrue);
    // Unticking one member takes the whole group out of the selection.
    await tester.tap(find.byType(Checkbox).first);
    await tester.pumpAndSettle();
    expect(tester.widgetList<Checkbox>(find.byType(Checkbox)).every((c) => c.value == false), isTrue);
    await tester.tapAt(const Offset(10, 10));
    await tester.pumpAndSettle();

    // The add-select toggle lights up.
    await tester.tap(find.byTooltip('追加選択'));
    await settle(tester, rounds: 10);
    final button = tester.widget<IconButton>(find.ancestor(of: find.byIcon(Icons.library_add_check_outlined), matching: find.byType(IconButton)));
    expect(button.style, isNotNull, reason: 'highlighted while on');

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
    expect(find.byTooltip('アップデート'), findsOneWidget, reason: 'updates live on the home screen');
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

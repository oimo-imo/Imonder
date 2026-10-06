import 'dart:io';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:imonder/storage/work_store.dart';

void main() {
  late Directory dir;
  late WorkStore store;

  setUp(() {
    dir = Directory.systemTemp.createTempSync('imonder_store_');
    store = WorkStore(dir);
  });
  tearDown(() => dir.deleteSync(recursive: true));

  test('create, save, load, list newest first', () async {
    final a = await store.create();
    await store.save(a, Uint8List.fromList([1, 2, 3]), thumbnailPng: Uint8List.fromList([9]));
    await Future<void>.delayed(const Duration(milliseconds: 5));
    final b = await store.create();
    await store.save(b, Uint8List.fromList([4]));
    expect(await store.load(a), [1, 2, 3]);
    expect(await store.load('nope'), isNull);
    final list = await store.list();
    expect(list.map((w) => w.id), [b, a]);
    expect(list.last.thumbnail, isNotNull);
    expect(list.first.thumbnail, isNull);
  });

  test('untitled names do not collide', () async {
    final a = await store.create();
    final b = await store.create();
    final c = await store.create();
    final names = (await store.list()).map((w) => w.name).toSet();
    expect(names, {'無題', '無題 2', '無題 3'});
    expect({a, b, c}.length, 3);
  });

  test('rename keeps the name across saves; blank names are ignored', () async {
    final a = await store.create();
    await store.rename(a, '  ねこ  ');
    await store.save(a, Uint8List.fromList([1]));
    expect((await store.list()).single.name, 'ねこ');
    await store.rename(a, '   ');
    expect((await store.list()).single.name, 'ねこ');
  });

  test('delete removes every file and forgets the last-opened work', () async {
    final a = await store.create();
    await store.save(a, Uint8List.fromList([1]), thumbnailPng: Uint8List.fromList([2]));
    await store.setLastOpened(a);
    expect(await store.lastOpened(), a);
    await store.delete(a);
    expect(await store.list(), isEmpty);
    expect(await store.lastOpened(), isNull);
    expect(dir.listSync().where((e) => e.path.contains(a)), isEmpty);
  });

  test('broken metadata is skipped, leftover temp files are ignored', () async {
    final a = await store.create();
    File('${dir.path}/zzz.json').writeAsStringSync('not json');
    File('${dir.path}/$a.imnd.tmp').writeAsBytesSync([1]);
    expect((await store.list()).map((w) => w.id), [a]);
  });
}

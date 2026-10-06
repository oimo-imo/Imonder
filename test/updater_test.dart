import 'package:flutter_test/flutter_test.dart';
import 'package:imonder/update/updater.dart';

void main() {
  test('version compare', () {
    expect(isNewer('v0.2.0', '0.1.0'), isTrue);
    expect(isNewer('v0.1.1', '0.1.0+5'), isTrue);
    expect(isNewer('v1.0.0', '0.9.9'), isTrue);
    expect(isNewer('v0.1.0', '0.1.0'), isFalse);
    expect(isNewer('v0.0.9', '0.1.0'), isFalse);
    expect(isNewer('nightly', '0.1.0'), isFalse);
  });

  test('asset selection', () {
    final assets = [
      {'name': 'imonder-windows-x64.zip'},
      {'name': 'imonder-android-arm64.apk'},
    ];
    expect(pickAsset(assets, android: true, windows: false)!['name'], 'imonder-android-arm64.apk');
    expect(pickAsset(assets, android: false, windows: true)!['name'], 'imonder-windows-x64.zip');
    expect(pickAsset(assets, android: false, windows: false), isNull);
  });
}

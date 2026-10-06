import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'ui/editor_screen.dart';

void main() {
  WidgetsFlutterBinding.ensureInitialized();
  SystemChrome.setEnabledSystemUIMode(SystemUiMode.edgeToEdge);
  runApp(const ImonderApp());
}

class ImonderApp extends StatelessWidget {
  const ImonderApp({super.key});

  @override
  Widget build(BuildContext context) {
    const accent = Color(0xFFF2A35E);
    return MaterialApp(
      title: 'Imonder',
      debugShowCheckedModeBanner: false,
      theme: ThemeData(
        useMaterial3: true,
        brightness: Brightness.dark,
        scaffoldBackgroundColor: const Color(0xFF1E1E21),
        colorScheme: ColorScheme.fromSeed(seedColor: accent, brightness: Brightness.dark).copyWith(primary: accent),
        fontFamilyFallback: const ['Hiragino Sans', 'Noto Sans JP', 'Yu Gothic UI', 'Meiryo'],
      ),
      home: const EditorScreen(),
    );
  }
}

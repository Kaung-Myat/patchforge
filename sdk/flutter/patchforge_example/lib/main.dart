import 'package:flutter/material.dart';
import 'package:patchforge_sdk/patchforge_sdk.dart';

const demoLabel = String.fromEnvironment(
  'PATCHFORGE_DEMO_LABEL',
  defaultValue: 'PATCHFORGE AUTO PATCH — v0.6',
);

void main() {
  runApp(const PatchForgeExample());
}

class PatchForgeExample extends StatelessWidget {
  const PatchForgeExample({super.key});

  @override
  Widget build(BuildContext context) {
    return const MaterialApp(
      debugShowCheckedModeBanner: false,
      home: HomePage(),
    );
  }
}

class HomePage extends StatefulWidget {
  const HomePage({super.key});

  @override
  State<HomePage> createState() => _HomePageState();
}

class _HomePageState extends State<HomePage> {
  String status = 'Not checked';
  bool loading = false;

  late final PatchForge patchForge;

  @override
  void initState() {
    super.initState();

    patchForge = PatchForge(
      config: PatchForgeConfig.fromEnvironment(
        bootStabilizationDelay: Duration(seconds: 5),
      ),
    );

    WidgetsBinding.instance.addPostFrameCallback((_) => refreshState());

    patchForge.bootConfirmation.then((result) {
      refreshState('Automatic boot confirmation: ${result.status.name}');
    });
  }

  Future<void> refreshState([String heading = 'Ready']) async {
    final state = await patchForge.getState();

    if (!mounted) return;

    setState(() {
      status =
          '$heading\n'
          'Current patch: ${state.currentPatch}\n'
          'Pending patch: ${state.pendingPatch}\n'
          'Loaded patch: ${state.loadedPatch}\n'
          'Status: ${state.status}';
    });
  }

  Future<void> checkUpdate() async {
    setState(() {
      loading = true;
      status = 'Checking...';
    });

    try {
      final result = await patchForge.update();

      if (!mounted) return;

      if (result.updated && result.newPatch != null) {
        final state = await patchForge.getState();

        if (!mounted) return;

        setState(() {
          status =
              'Patch ${result.newPatch} reconstructed and staged\n'
              'Binary: ${result.file?.path}\n'
              'Current patch: ${state.currentPatch}\n'
              'Pending patch: ${state.pendingPatch}\n'
              'Loaded patch: ${state.loadedPatch}\n'
              'Restart the release app to activate this patch.\n'
              'Status: ${state.status}';
        });
      } else {
        final state = await patchForge.getState();

        setState(() {
          status =
              'No update available\n'
              'Current patch: ${state.currentPatch}\n'
              'Loaded patch: ${state.loadedPatch}\n'
              'Status: ${state.status}';
        });
      }
    } catch (error) {
      if (!mounted) return;

      setState(() {
        status = 'Error: $error';
      });
    } finally {
      if (mounted) {
        setState(() {
          loading = false;
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('PatchForge')),
      body: Center(
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(
                demoLabel,
                style: Theme.of(context).textTheme.headlineSmall,
                textAlign: TextAlign.center,
              ),
              const SizedBox(height: 20),
              Text(status, textAlign: TextAlign.center),
              const SizedBox(height: 20),
              ElevatedButton(
                onPressed: loading ? null : checkUpdate,
                child: Text(loading ? 'Checking...' : 'Check Update'),
              ),
              const SizedBox(height: 12),

              ElevatedButton(
                onPressed: () async {
                  await patchForge.rollback();

                  final state = await patchForge.getState();

                  if (!mounted) return;

                  setState(() {
                    status =
                        'Rollback complete\n'
                        'Current patch: ${state.currentPatch}\n'
                        'Pending patch: ${state.pendingPatch}\n'
                        'Status: ${state.status}';
                  });
                },
                child: const Text('Rollback'),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

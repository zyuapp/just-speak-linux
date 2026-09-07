"""Regression checks for release URLs and compatibility with old OTA clients."""
import json
from pathlib import Path
import tempfile
import unittest

from package_ui import stage_ui


class PackageUiTests(unittest.TestCase):
    def test_releases_change_every_local_component_url(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / 'source'
            source.mkdir()
            (source / 'Widget.qml').write_text('import QtQuick\nItem { DictationMenu {} }\n')
            (source / 'DictationMenu.qml').write_text('import QtQuick\nItem { property string marker: "old" }\n')
            (source / 'shell.qml').write_text('import QtQuick\nWidget {}\n')
            for version in ['0.2.1', '0.2.2']:
                (source / 'manifest.json').write_text(json.dumps({'version': version, 'entryPoints': {'barWidget': 'Widget.qml'}}))
                stage_ui(source, root / version, version)
            old = root / '0.2.1'
            new = root / '0.2.2'
            self.assertNotEqual(json.loads((old / 'manifest.json').read_text())['entryPoints'],
                                json.loads((new / 'manifest.json').read_text())['entryPoints'])
            self.assertIn('DictationMenuV0_2_2 {}', (new / 'WidgetV0_2_2.qml').read_text())
            self.assertTrue((new / 'DictationMenuV0_2_2.qml').is_file())
            for name in ['Widget.qml', 'DictationMenu.qml', 'shell.qml']:
                self.assertEqual((new / name).read_bytes(), (source / name).read_bytes())
            # v0.2.0/0.2.1 updaters permit only flat QML and manifest filenames.
            self.assertTrue(all(path.is_file() and (path.suffix == '.qml' or path.name == 'manifest.json')
                                for path in new.iterdir()))

    def test_actual_component_graph_has_no_unversioned_local_type_references(self):
        source = Path(__file__).resolve().parent.parent / 'ui'
        version = json.loads((source / 'manifest.json').read_text())['version']
        suffix = 'V' + version.replace('.', '_')
        with tempfile.TemporaryDirectory() as temporary:
            destination = Path(temporary)
            stage_ui(source, destination, version)
            manifest = json.loads((destination / 'manifest.json').read_text())
            self.assertTrue((destination / manifest['entryPoints']['barWidget']).is_file())
            for component in source.glob('[A-Z]*.qml'):
                staged = (destination / (component.stem + suffix + '.qml')).read_text()
                for other in source.glob('[A-Z]*.qml'):
                    self.assertNotRegex(staged, r'\b' + other.stem + r'\b')


if __name__ == '__main__':
    unittest.main()

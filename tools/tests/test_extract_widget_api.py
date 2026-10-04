# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('extract_widget_api', ROOT / 'tools/extract_widget_api.py')
api = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = api
spec.loader.exec_module(api)


class TraitExtractionTests(unittest.TestCase):
    def test_public_trait_contract_and_default_body(self):
        source = '''
/// Source contract.
#[cfg(feature = "source")]
pub unsafe trait Source<T>: Send
where T: Clone {
    /// Payload description.
    type Payload: Send;
    const LIMIT: usize = 8;
    fn required(&self, value: T) -> Self::Payload;
    #[doc(hidden)]
    unsafe fn optional(&self) {
        fn nested() {}
    }
}
impl Source<i32> for Adapter {
    fn required(&self, value: i32) -> i32 { value }
}
'''
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'source.rs'
            path.write_text(source)
            parsed = api.parse_file(path, 'source', [])
        trait, = parsed.items
        self.assertEqual(trait.kind, 'trait')
        self.assertEqual(trait.doc, 'Source contract.')
        self.assertIn('where T: Clone', trait.signature)
        self.assertEqual(trait.cfg, ['#[cfg(feature = "source")]'])
        self.assertEqual([m.name for m in trait.methods], ['Payload', 'LIMIT', 'required', 'optional'])
        self.assertEqual(trait.methods[0].doc, 'Payload description.')
        self.assertTrue(trait.methods[2].signature.endswith(';'))
        self.assertIn('default implementation', trait.methods[3].signature)
        self.assertTrue(trait.methods[3].hidden)
        self.assertEqual(json.loads(api.format_json([parsed]))[0]['items'][0]['kind'], 'trait')

    def test_real_public_traits_include_their_contracts(self):
        cases = [('event_source.rs', 'EventSource', {'Origin', 'Event', 'subscribe'}),
                 ('event_source.rs', 'AppEventPoster', {'post_subscription_event', 'post_external'}),
                 ('widget.rs', 'Widget', {'build', 'layout_response', 'paint'})]
        for filename, name, expected in cases:
            with self.subTest(name=name):
                parsed = api.parse_file(ROOT / 'crates/teksilo-core/src' / filename, filename, [])
                trait = next(item for item in parsed.items if item.name == name)
                self.assertTrue(expected <= {member.name for member in trait.methods})


if __name__ == '__main__':
    unittest.main()

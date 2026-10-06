#!/usr/bin/env python3
"""Check curated C declarations against the actual exported library symbols."""
import pathlib,re,subprocess,sys
root=pathlib.Path(__file__).resolve().parents[2]
library=pathlib.Path(sys.argv[1])
headers=root/'crates/ttl-live-native/include/ttl'
expected=set()
for p in headers.glob('*.h'):
    expected.update(re.findall(r'TTL_API\s+[^;]+?\b(ttl_\w+)\s*\(',p.read_text()))
if sys.platform == 'darwin':
    output=subprocess.check_output(['nm','-gU',str(library)],text=True)
else:
    output=subprocess.check_output(['nm','-D','--defined-only',str(library)],text=True)
actual=set(re.findall(r'\b_?(ttl_\w+)\s*$',output,re.M))
missing=expected-actual
extra=actual-expected
if missing or extra:
    raise SystemExit(f'ABI export mismatch: missing={sorted(missing)}, undocumented={sorted(extra)}')
print(f'{len(expected)} curated declarations match native library exports')

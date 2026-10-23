#!/usr/bin/env python3
"""Unit 1 G1: source generated strict inventory of Runtime API routes."""
import json,re
from pathlib import Path
root=Path(__file__).resolve().parents[1]
runtime=(root/'crates/runtime/src/lib.rs').read_text()
section=runtime.split('match (request.method.as_str(), request.path.as_str()) {',1)[1].split('async fn handle_write_stream_route(',1)[0]
found=set(re.findall(r'\("(GET|POST|PUT|DELETE|PATCH)",\s*"(/v1/[^\"]+)"\)\s*=>|\("(GET|POST|PUT|DELETE|PATCH)",\s*"(/v1/[^\"]+)"\)\s*\|',section))
literal=set()
for match in re.finditer(r'\("(GET|POST|PUT|DELETE|PATCH)",\s*"(/v1/[^\"]+)"\)\s*(?:=>|\|)',section):
    literal.add((match.group(1),match.group(2)))
# Alternation's last arm has =>; covered by regex. Match may also contain
# routes in comments, so verify every entry is an actual match token.
assert len(literal) > 35, f'route extractor only got {len(literal)}'
primary_paths='''/v1/spaces/open
/v1/spaces/clear
/v1/kv/set
/v1/fs/write-small
/v1/fs/delete
/v1/fs/copy
/v1/fs/move
/v1/batch
/v1/batch/resume
/v1/streams/write/begin
/v1/formats/register
/v1/formats/delete
/v1/destinations/create
/v1/destinations/revoke
/v1/export-presets/save
/v1/export-presets/delete
/v1/exports/start
/v1/exports/resume
/v1/imports/start
/v1/imports/resume
/v1/snapshots/create
/v1/snapshots/restore
/v1/snapshots/delete'''.split()
reads='''/v1/operations/status
/v1/streams/write/capability
/v1/streams/read/begin
/v1/spaces/list
/v1/kv/get
/v1/formats/list
/v1/destinations/list
/v1/export-presets/list
/v1/snapshots/list
/v1/events/poll
/v1/fs/stat
/v1/fs/read-small
/v1/fs/read-many'''.split()
bootstrap='''/v1/health
/v1/identity/challenge
/v1/pairings/request
/v1/pairings/poll
/v1/sessions'''.split()
cancel=['/v1/operations/cancel']
classification={}
for group,paths in [('REQUIRED_PRIMARY',primary_paths),('EXCLUDED_READ_CONTROL',reads),('EXCLUDED_BOOTSTRAP',bootstrap),('AUXILIARY_CANCEL',cancel)]:
    for path in paths:
        assert path not in classification,path
        classification[path]=group
assert len(classification)==len(literal),(len(classification),len(literal))
assert {(('GET' if p=='/v1/health' else 'POST'),p) for p in classification}==literal,sorted(literal-{(('GET' if p=='/v1/health' else 'POST'),p) for p in classification})
# Dynamic branches are dispatched ahead of the static match.
dynamic_section=runtime.split('async fn handle_write_stream_route(',1)[1].split('async fn handle_read_stream_route(',1)[0]
assert 'strip_prefix("/v1/streams/write/")' in dynamic_section
assert 'parts[0] == "resolve"' in dynamic_section
assert 'parts[1] == "status"' in dynamic_section
for marker in ['parts[1] == "commit"','parts[1] == "abort"','parts[2] == "chunk"','request.method == "PUT"']:
    assert marker in dynamic_section,marker
assert 'strip_prefix("/v1/streams/read/")' in runtime.split('async fn handle_read_stream_route(',1)[1]
dynamic=[
    ('POST','/v1/streams/write/resolve','EXCLUDED_READ_CONTROL'),
    ('GET','/v1/streams/write/{streamId}/status','EXCLUDED_READ_CONTROL'),
    ('PUT','/v1/streams/write/{streamId}/{seq}','STREAM_CHUNK_SUBSTEP'),
    ('POST','/v1/streams/write/{streamId}/{seq}/chunk','STREAM_CHUNK_SUBSTEP'),
    ('POST','/v1/streams/write/{streamId}/commit','REQUIRED_PRIMARY'),
    ('POST','/v1/streams/write/{streamId}/abort','REQUIRED_PRIMARY'),
    ('GET','/v1/streams/read/{streamId}/{seq}','EXCLUDED_READ_CONTROL'),
]
entries=[{'method':m,'path':p,'classification':classification[p],'dispatch':'literal'} for m,p in sorted(literal)]
entries.extend({'method':m,'path':p,'classification':c,'dispatch':'dynamic'} for m,p,c in dynamic)
output={'source':'VontaqFS/crates/runtime/src/lib.rs','method':'literal route arm regex + dynamic branch check',
    'counts':{'literal':len(literal),'dynamic':len(dynamic),'total':len(entries),
              'requiredPrimary':sum(x['classification']=='REQUIRED_PRIMARY' for x in entries)},'routes':entries}
(target:=root/'docs/architecture/CP14_FINITE_ROUTE_INVENTORY.json').write_text(json.dumps(output,ensure_ascii=False,indent=2)+'\n')
print('PASS CP14 G1 strict route inventory',output['counts'])

const assert = require('node:assert/strict');
const readline = require('node:readline');
const fs = require('node:fs');
const vm = require('node:vm');
const {Client, Identifier} = require(process.argv[2]);
const lines = readline.createInterface({input:process.stdin});
const pending = [];
lines.on('line', line => pending.shift()(JSON.parse(line)));
const send = request => new Promise(resolve => { pending.push(resolve); process.stdout.write(JSON.stringify(request) + '\n'); });
const client = new Client(send);
const packet = {text:'one\ntwo', bytes:new Uint8Array([0,1,127,128,255]), integer:-9223372036854775808n, number:1.25, flag:{kind:'true'}, items:[9007199254740993n,9223372036854775807n], dict:{a:'b'}, identifier:Identifier(42n), selection:{kind:'selected',item:'x'}};
(async () => {
  const result = await client.echo(packet);
  assert.equal(result.kind, 'value');
  assert.deepEqual({...result.value, dict:{...result.value.dict}}, packet);
  for (const number of [NaN,Infinity,-Infinity]) await assert.rejects(client.echo({...packet,number}));
  for (const integer of [-9223372036854775809n,9223372036854775808n,42]) await assert.rejects(client.echo({...packet,integer}));
  for (const response of [
    'RETURNS {}',
    'RETURNS { text "x", bytes "AA=A", integer 1, number 1.0, flag true, items [], dict {}, identifier 1, selection none }',
    'RETURNS { text "x", bytes "AA==", integer 1, number 1e999, flag true, items [], dict {}, identifier 1, selection none }',
    'ERROR unknown', 'RETURNS {} , RETURNS {}', 'RETURNS {',
  ]) await assert.rejects(new Client(async () => response).echo(packet));
  const errors = await client.batch(c => [c.echo(packet), c.echo({...packet,integer:5n})]);
  assert.equal(errors[0].value.integer, packet.integer);
  assert.equal(errors[1].value.integer, 5n);
  // The emitted module needs only standard ECMAScript globals. Exercise it in
  // isolated browser/edge-like contexts without Node globals or Buffer.
  for (const surface of ['browser', 'edge']) {
    const context = vm.createContext({exports: {}});
    vm.runInContext(fs.readFileSync(process.argv[2], 'utf8'), context, {filename: surface});
    const result = await vm.runInContext(`new exports.Client(async () => 'RETURNS "ok"').schema()`, context);
    assert.equal(result.value, 'ok');
  }
  lines.close();
})().catch(error => {console.error(error); process.exitCode=1; lines.close();});

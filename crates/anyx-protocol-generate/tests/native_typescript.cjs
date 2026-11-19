const assert = require('node:assert/strict');
const readline = require('node:readline');
const {Client, unwrapOutcome, OutcomeFailure} = require(process.argv[2]);
const lines = readline.createInterface({input: process.stdin});
const pending = [];
lines.on('line', line => pending.shift()(JSON.parse(line)));
const send = request => new Promise(resolve => { pending.push(resolve); process.stdout.write(JSON.stringify(request) + '\n'); });
const client = new Client(send);
const at = new Date('2026-11-14T06:00:08.000Z');
const reminder = {note: 'tea', at, times: 9007199254740993n, seen: {}};
(async () => {
  const result = await client.count({tea: reminder});
  assert.equal(unwrapOutcome(result).tea, 9007199254740993n);
  const largestDuration = 18446744073709551615n;
  assert.equal((await client.snoozeFor(reminder, largestDuration)).kind, 'error');
  const duration = await new Client(async () => 'RETURNS `every (18446744073709551615 milliseconds)').remind(reminder);
  assert.equal(duration.value.period, largestDuration);
  await assert.rejects(client.snoozeFor(reminder, largestDuration + 1n));
  const declared = await client.snoozeFor(reminder, 1000n);
  assert.equal(declared.kind, 'error');
  assert.equal(declared.error.kind, 'overdueSince');
  assert.throws(() => unwrapOutcome(declared), OutcomeFailure);
  // A field with several slots is an array of their values.
  let sent;
  const quiet = await new Client(async request => { sent = String(request); return 'RETURNS [{ note "tea", at "2026-11-14T06:00:08.000Z", quiet from 23 to 6 }, { note "tea", at "2026-11-14T06:00:08.000Z" }]'; })
    .snoozeFor({...reminder, quietFromTo: [1n, 5n]}, 1000n);
  assert.deepEqual(quiet.value.map(r => r.quietFromTo), [[23n, 6n], [22n, 7n]]);
  assert.match(sent, /quiet from \(1\) to \(5\)/);
  const batch = await client.batch(c => [c.remind(reminder), c.count({tea: reminder})]);
  assert.equal(batch[0].value.kind, 'onceAt');
  assert.equal(batch[1].value.tea, reminder.times);
  for (const response of ['RETURNS 1', 'RETURNS {} , RETURNS {}', 'ERROR made up', 'RETURNS { "x" 1e999 }', 'RETURNS { "x" 9223372036854775808 }']) {
    await assert.rejects(new Client(async () => response).count({}));
  }
  await assert.rejects(client.count({tea: {...reminder, times: 9223372036854775808n}}));
  await assert.rejects(new Client(async () => { throw Error('transport'); }).count({}));
  const protocol = new Client(async () => 'INTERNAL ERROR, NOT RUN,');
  const stopped = await protocol.batch(c => [c.count({}), c.count({})]);
  assert.deepEqual(stopped.map(x => x.kind), ['internalError', 'notRun']);
  const rejected = new Client(async () => 'INVALID REQUEST { line 1, column 2, message "bad" }');
  const invalid = await rejected.batch(c => [c.count({}), c.count({})]);
  assert.deepEqual(invalid.map(x => [x.kind, x.line, x.column, x.message]), [['invalidRequest',1,2,'bad'], ['invalidRequest',1,2,'bad']]);
  for (const [response, expected] of [
    ['RETURNS: "hello"', 'hello'],
    ['"hello" . RETURNS _', 'hello'],
    ['RETURNS "\\u0041\\uD83D\\uDE00"', 'A😀'],
    ['RETURNS """\nhello\nworld\n"""', 'hello\nworld'],
  ]) { try { assert.equal((await new Client(async () => response).schema()).value, expected); } catch (error) { throw new Error(`response ${JSON.stringify(response)}: ${error}`); } }
  lines.close();
})().catch(error => { console.error(error); process.exitCode = 1; lines.close(); });

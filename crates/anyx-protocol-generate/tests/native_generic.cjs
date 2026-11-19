const assert = require('node:assert/strict');
const {Client} = require(process.argv[2]);
(async () => {
  const responses = [
    'RETURNS { value "root", children [{ value "leaf", children [] }] }',
    'RETURNS { items [{ amount 9223372036854775807, at "2026-11-14T06:00:08Z" }] }',
    'RETURNS failed (insufficient funds)',
    'RETURNS { items [], next "2" }',
  ];
  const client = new Client(async () => responses.shift());
  const tree = await client.categories();
  assert.equal(tree.value.children[0].value, 'leaf');
  const page = await client.payments();
  assert.equal(page.value.total, 0n);
  assert.equal(page.value.items[0].amount, 9223372036854775807n);
  // A nullable field left out holds its default, `null`.
  assert.deepEqual(page.value.next, {kind: 'null'});
  const attempt = await client.pay(1n);
  assert.equal(attempt.value.kind, 'failed');
  assert.equal(attempt.value.problem.kind, 'insufficientFunds');
  // A nullable value is written as its item.
  assert.equal((await client.payments()).value.next, '2');
})().catch(error => {console.error(error); process.exitCode = 1;});

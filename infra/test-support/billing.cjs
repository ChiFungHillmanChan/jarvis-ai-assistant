// Run the production Lambda entry points with in-memory AWS/Stripe boundaries.
// Unexpected imports fail closed; no credential provider or network client loads.
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const crypto = require('node:crypto');
const ts = require('typescript');
const Stripe = require('../lambda/stripe-webhook/node_modules/stripe');

exports.createHarness = function createHarness(overrides = {}) {
  const rows = new Map();
  const customers = new Map();
  const subscriptions = new Map();
  const sessions = new Map();
  const invoices = new Map();
  const calls = [];
  const idempotent = new Map();
  const env = { ENV: 'sandbox', USER_META_TABLE: 'fixture-users', USER_POOL_ID: 'fixture-pool', STRIPE_SECRET_NAME: 'fixture-secret', APP_DOMAIN: 'example.test', ...overrides };
  const secret = 'whsec_synthetic_local_fixture';
  let counter = 0;
  let failUpdateOnce = false;
  let failUpdateMatching;
  const copy = value => value === undefined ? undefined : JSON.parse(JSON.stringify(value));
  function conditionalFailure() { const error = new Error('Condition failed'); error.name = 'ConditionalCheckFailedException'; throw error; }
  class GetCommand { constructor(input) { this.input = input; } }
  class UpdateCommand { constructor(input) { this.input = input; } }
  const db = { async send(command) {
    const p = command.input;
    calls.push({ kind: command.constructor.name, input: copy(p) });
    if (command instanceof GetCommand) return { Item: copy(rows.get(p.Key.userId)) };
    if (failUpdateMatching && p.UpdateExpression.includes(failUpdateMatching)) { failUpdateMatching = undefined; throw new Error('Synthetic temporary database failure'); }
    const row = copy(rows.get(p.Key.userId) || {});
    const values = p.ExpressionAttributeValues || {};
    if (p.ConditionExpression?.includes('attribute_not_exists(billingLockUntil)') && row.billingLockUntil !== undefined && row.billingLockUntil >= values[':now']) conditionalFailure();
    if (p.ConditionExpression === 'billingLockOwner = :owner' && row.billingLockOwner !== values[':owner']) conditionalFailure();
    if (p.ConditionExpression === 'attribute_not_exists(stripeCustomerId)' && row.stripeCustomerId !== undefined) conditionalFailure();
    if (failUpdateOnce && p.UpdateExpression.includes('SET tier')) { failUpdateOnce = false; throw new Error('Synthetic temporary database failure'); }
    const set = p.UpdateExpression.match(/SET (.*?)(?: REMOVE |$)/)?.[1];
    for (const assignment of set?.split(',') || []) {
      const [key, value] = assignment.trim().split(/\s*=\s*/);
      row[key] = copy(values[value]);
    }
    const remove = p.UpdateExpression.match(/REMOVE (.*)$/)?.[1];
    for (const key of remove?.split(',') || []) delete row[key.trim()];
    rows.set(p.Key.userId, row);
    return { Attributes: copy(row) };
  } };
  async function once(kind, input, options, create) {
    calls.push({kind,input:copy(input),options:copy(options)});
    const key = options?.idempotencyKey;
    if (key && idempotent.has(key)) {
      const previous = idempotent.get(key);
      if (previous.input !== JSON.stringify(input)) throw new Error('Idempotency parameters changed');
      return copy(previous.result);
    }
    const result = create();
    if (key) idempotent.set(key, {input:JSON.stringify(input),result});
    return copy(result);
  }
  const stripe = {
    webhooks: Stripe.webhooks,
    customers: {
      create: (input, options) => once('customer.create',input,options,()=>{ const customer={id:`cus_${++counter}`,...input};customers.set(customer.id,customer);return customer; }),
      retrieve: async id => { if(!customers.has(id)) throw new Error('Unknown fixture customer');return copy(customers.get(id)); },
    },
    subscriptions: {
      retrieve: async id => { if(!subscriptions.has(id)) throw new Error('Unknown fixture subscription');return copy(subscriptions.get(id)); },
      list: async input => ({data:copy([...subscriptions.values()].filter(sub=>sub.customer===input.customer && (input.status==='all' || sub.status!=='canceled'))),has_more:false}),
    },
    invoices: { retrieve: async id => { if(!invoices.has(id)) throw new Error('Unknown fixture invoice');return copy(invoices.get(id)); } },
    checkout: { sessions: {
      create: (input, options) => once('checkout.create',input,options,()=>{const session={id:`cs_${++counter}`,url:`https://checkout.stripe.com/c/pay/${counter}`,status:'open',payment_status:'unpaid',...input};sessions.set(session.id,session);return session;}),
      list: async input => ({data:copy([...sessions.values()].filter(session=>session.customer===input.customer && (!input.status || session.status===input.status))),has_more:false}),
      retrieve: async id => {if(!sessions.has(id)) throw new Error('Unknown fixture session');return copy(sessions.get(id));},
      expire: async id => {const session=sessions.get(id);if(!session||session.status!=='open')throw new Error('Session no longer open');session.status='expired';calls.push({kind:'checkout.expire',id});return copy(session);},
    } },
    billingPortal: { sessions: { create: async input => {calls.push({kind:'portal.create',input:copy(input)});return {id:`bps_${++counter}`,url:'https://billing.stripe.com/p/session/fixture'};} } },
  };
  class SecretsClient { async send() { return {SecretString:JSON.stringify({apiKey:'synthetic-only',webhookSecret:secret})}; } }
  class CognitoClient { async send() { return {UserAttributes:[{Name:'email',Value:'buyer@example.test'}]}; } }
  class Command {constructor(input){this.input=input;}}
  class FakeStripe {constructor(){return stripe;}}
  const mocks = {
    '@aws-sdk/client-secrets-manager':{SecretsManagerClient:SecretsClient,GetSecretValueCommand:Command},
    '@aws-sdk/client-dynamodb':{DynamoDBClient:class{}},
    '@aws-sdk/lib-dynamodb':{DynamoDBDocumentClient:{from:()=>db},GetCommand,UpdateCommand},
    '@aws-sdk/client-cognito-identity-provider':{CognitoIdentityProviderClient:CognitoClient,AdminGetUserCommand:Command},
    'stripe':FakeStripe,
    'node:crypto':crypto,
  };
  const loaded = new Map();
  function load(file) {
    if (loaded.has(file)) return loaded.get(file).exports;
    const module = {exports:{}};loaded.set(file,module);
    const js = ts.transpileModule(fs.readFileSync(file,'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022,esModuleInterop:true}}).outputText;
    vm.runInNewContext(js,{module,exports:module.exports,require(name){if(name in mocks)return mocks[name];if(name.startsWith('.'))return load(path.resolve(path.dirname(file),name.replace(/\.ts$/,'')+'.ts'));throw new Error('Unexpected dependency: '+name);},process:{env},Buffer,console:{log(){},warn(){},error(){}}},{filename:file});
    return module.exports;
  }
  const request = (tier='pro') => ({headers:{},requestContext:{authorizer:{jwt:{claims:{sub:'user_fixture',username:'user_fixture'}}}},body:JSON.stringify({tier}),isBase64Encoded:false});
  function webhookEvent(type,object,id='evt_fixture') {
    const body=JSON.stringify({id,type,created:1,data:{object}});
    return {headers:{'stripe-signature':Stripe.webhooks.generateTestHeaderString({payload:body,secret})},body,isBase64Encoded:false};
  }
  function seedSubscription(status='active', id='sub_fixture', price='price_1TUsAuJCG4qg3rNcZVpOopcD') {
    customers.set('cus_fixture',{id:'cus_fixture',metadata:{userId:'user_fixture',project:'jarvis'}});
    rows.set('user_fixture',{stripeCustomerId:'cus_fixture'});
    const subscription={id,customer:'cus_fixture',status,metadata:{userId:'user_fixture',project:'jarvis'},items:{data:[{price:{id:price}}]},latest_invoice:'in_fixture',created:1};
    subscriptions.set(id,subscription);
    invoices.set('in_fixture',{id:'in_fixture',customer:'cus_fixture',subscription:id,paid:true,status:'paid'});
    return subscription;
  }
  return { rows,customers,subscriptions,sessions,invoices,calls,env,stripe,request,webhookEvent,seedSubscription,
    handler:name=>load(path.join(__dirname,'../lambda',name,'index.ts')).handler,
    failNextTierUpdate(){failUpdateOnce=true;},
    failNextUpdateContaining(expression){failUpdateMatching=expression;},
  };
};

# @ckb/sdk

A minimal Node.js client for the CKB MCP REST server (`ckb-mcp-server`). Requires
Node 18+ (uses the built-in `fetch`) — no native build step.

## Usage

```js
const { CkbClient } = require('@ckb/sdk');

const client = new CkbClient({
  baseUrl: 'http://localhost:3000',
  apiKey: process.env.CKB_API_KEY, // only needed if the server was started with CKB_API_KEY set
});

const report = await client.scan('/path/to/your/repo');
console.log(`Found ${report.violations_found} architecture violations`);

const impact = await client.analyzeImpact({
  path: '/path/to/your/repo',
  file: 'src/payments/checkout.ts',
  line: 42,
  changeType: 'modify',
});
```

## API

See `index.d.ts` for full types. All methods return the parsed JSON response
and throw `CkbApiError` (with `.status` and `.body`) on a non-2xx response.

## Starting the server this client talks to

```bash
CKB_API_KEY=your-key ckb-mcp-server
```

By default the server binds to `127.0.0.1:3000`. See the root `README.md` and
`mcp-server/` for server configuration.

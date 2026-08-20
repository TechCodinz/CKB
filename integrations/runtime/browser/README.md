# CKB Browser Live Reality

First-party browser collector for the CKB Live Execution Twin.

## Security model

Browser bundles are public artifacts. **Do not embed a `ckb_live_*`, internal Reality secret, or other long-lived CKB credential in frontend JavaScript.**

`CkbBrowserRuntime` therefore requires an application-controlled `exporter(payload)` callback. In production that exporter should send the OTLP JSON payload to your own authenticated backend (or another approved short-lived telemetry broker), which then forwards it to CKB.

The collector never needs the CKB Reality engine credential.

## Evidence captured

- exact trace/span ids
- parent/child trace context
- manually named browser operations
- outbound HTTP boundaries
- stable developer-supplied route templates
- response status/error state
- allowlisted scalar custom attributes

## Privacy defaults

The browser collector does **not** automatically export:

- raw URL paths or query strings
- request/response bodies
- cookies or authorization headers
- session/user tokens
- email/phone/address fields
- SQL/payload values

For an outbound request, the automatic network identity is the origin (`https://api.example.com`). If you want source-aware routing, pass a stable route template such as `/orders/{id}`.

```js
import { CkbBrowserRuntime } from './ckb-browser.mjs';

const ckb = new CkbBrowserRuntime({
  serviceName: 'storefront-web',
  exporter: async payload => {
    await fetch('/internal/observability/ckb', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(payload),
    });
  },
}).start();

await ckb.fetch('https://api.example.com/orders/123?token=private', {}, {
  route: '/orders/{id}',
});
```

The application backend is responsible for authenticating that forwarding endpoint and applying its own abuse/rate limits before forwarding telemetry to CKB.

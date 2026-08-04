# ckb-sdk (Python)

A minimal, dependency-free Python client for the CKB MCP REST server
(`ckb-mcp-server`). Uses only the standard library (`urllib`) — nothing to
`pip install` beyond this package itself.

## Usage

```python
from ckb import CkbClient

client = CkbClient(base_url="http://localhost:3000", api_key="your-key")  # api_key optional

report = client.scan("/path/to/your/repo")
print(f"Found {report['violations_found']} architecture violations")

impact = client.analyze_impact(
    path="/path/to/your/repo",
    file="src/payments/checkout.py",
    line=42,
    change_type="modify",
)
```

## Errors

All methods raise `ckb.CkbApiError` (with `.status` and `.body`) on a
non-2xx response or connection failure.

## Starting the server this client talks to

```bash
CKB_API_KEY=your-key ckb-mcp-server
```

By default the server binds to `127.0.0.1:3000`. See the root `README.md`
and `mcp-server/` for server configuration.

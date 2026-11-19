# Challenge solvers and browser sidecars

Normal turbo-surf navigation is browserless: Rust networking and a V8 isolate run
without Chrome. Challenge solving is optional. A configured external provider or
browser command can introduce a separate service or Chrome process; it does not
replace the native engine. Browser-backed search is a separate configuration.

## Selection and configuration

MCP constructs its solver when `Session::new` runs. The core loader reads the
first existing `.env` among `.env`, `../.env`, `../../.env`, and `../../../.env`,
relative to the process working directory. Existing process variables win. The
parser accepts `KEY=value`, trims whitespace and surrounding double quotes, and
skips blank/comment lines; it does not implement shell expansion, `export`, or
full dotenv quoting. `.env.local` is not searched. Restart the session after
changing configuration.

`TURBO_SURF_SOLVER` is case-insensitive. These are the implemented selections:

| Value | Requirements and current boundary |
| --- | --- |
| `hyper` | `HYPER_API_KEY`; the adapter implements Hyper's Akamai sensor-generation and submission flow. Other detected vendors return unsupported. |
| `scrapfly` | `SCRAPFLY_API_KEY`; requests ASP with JS rendering and imports returned cookies. Provider acceptance is external to turbo-surf. |
| `browser` | Nonblank `TURBO_SURF_BROWSER_CMD`; invokes the challenge sidecar described below. |
| `akamai` | No provider key. Experimental native sensor construction; not a general live Akamai guarantee. |
| `cloudflare` | No provider key. Parses recognized challenge parameters, optionally computes extracted inline challenge JS in V8, and submits for `cf_clearance`. Structural answer fallback remains when computation is unavailable. This is not universal managed-challenge or interactive Turnstile support. |
| `awswaf` or `aws` | No provider key. Extracts inline challenge JS for a V8 token; otherwise returns a structural placeholder token. CAPTCHA-tier pages return unsupported. Live edge acceptance is not established by local fixtures. |
| `recaptcha` | No provider key. MCP injects the V8 token-flow engine. Visual image-grid challenges return an error; producing a token does not establish server acceptance or a sufficient score. |
| Empty, unset, or any other value | Selects the first usable provider key: Hyper, then Scrapfly; otherwise no solver. An unknown value does not disable solving when keys exist. |

Only `HYPER_API_KEY` and `SCRAPFLY_API_KEY` are provider-key inputs. Empty keys,
keys beginning `your_`, and keys containing `_here` count as unset. An explicit
`hyper`/`scrapfly` selection with a missing key gives no solver; it does not try
the other provider. The plain core `solver_from_env()` does not inject V8 engines;
MCP uses `solver_from_env_pow()` with both engines.

For each in-house selection (`akamai`, `cloudflare`, `awswaf`/`aws`, `recaptcha`),
a nonblank `TURBO_SURF_BROWSER_CMD` wraps it in browser fallback. The browser runs
only when the primary solver returns an error. A returned token later rejected
by the target does not itself trigger that fallback. Merely setting the browser
command with no in-house or explicit `browser` selection does not select it.

Launch from the repository root, for example:

```bash
TURBO_SURF_SOLVER=cloudflare node cli.js
```

For provider use, supply the corresponding real key through the environment or a
local `.env`; never put it in a committed example. See the [challenge-sidecar
setup](../harness/browser-solver/README.md) for explicit browser selection and
optional native-first fallback.

## Status and navigation behavior

MCP `stealth_status` returns `solver: null`, the selected solver's name, or
`"fallback"` for a wrapped in-house solver. This describes configuration, not
provider health or a successful solve. `solve_recaptcha` is a separate MCP tool
that invokes the V8 reCAPTCHA flow independently of the environment selection.

On a detected navigation wall, MCP tries the configured solver once. A solve
error leaves the original challenge page in place. On success it installs
returned cookie name/value pairs at the target URL with `Path=/`, adds returned
headers to the session, and refetches once. Cookie metadata is not preserved by
this envelope. Although solver results carry a TTL, this MCP path does not turn
it into an explicit cookie expiry or a separate token cache.

`TURBO_SURF_PROXY` is passed as a solver-context hint. The browser solver forwards
it to its command; Scrapfly uses its presence to select a residential pool, not
the supplied proxy URL. It is not a native replay proxy configuration: the
native networking code does not consume this variable. Do not assume that
setting it makes solver and native replay egress identical. IP, user-agent, TLS
and other binding checks can still reject a token.

## Two distinct browser commands

| Variable | Purpose | Command parsing |
| --- | --- | --- |
| `TURBO_SURF_BROWSER_CMD` | Challenge token sidecar: URL/vendor/UA/proxy in; cookie/header maps out. | Executed through `sh -c`; shell quoting applies. |
| `TURBO_SURF_BROWSER_FETCH_CMD` | Search fetch or ENID cookie mint: URL or mint request in; HTML or cookie array out. | Split on whitespace into executable and arguments; shell quoting/expansion is not interpreted. Use a wrapper executable when paths contain spaces. |

Both are opt-in external processes, not a Chrome dependency of the normal
engine. Their JSON envelopes are incompatible. See [search-sidecar setup and
limitations](../scripts/browser-sidecar/README.md).

## Evidence boundary

Local Rust fixtures exercise detection, solver adapters, fallback, and replay;
they do not prove current live Cloudflare, AWS WAF, Akamai, CAPTCHA, Google, or
provider acceptance. The reference browser solver collects cookies after a
navigation and short wait; it neither confirms clearance nor solves an image
grid. Browser automation, a provider key, or a successful status field alone
cannot establish target acceptance.

Implementation: [selection and sidecar contract](../rust/crates/turbo-surf-core/src/challenge.rs),
[MCP session integration](../rust/crates/turbo-surf-mcp/src/lib.rs),
[Cloudflare](../rust/crates/turbo-surf-core/src/cloudflare.rs),
[AWS WAF](../rust/crates/turbo-surf-core/src/aws_waf.rs), and
[reCAPTCHA](../rust/crates/turbo-surf-core/src/recaptcha.rs).

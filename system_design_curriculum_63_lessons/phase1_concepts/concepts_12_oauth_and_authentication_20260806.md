# System Design Mentor — Daily Lesson
**Date:** 06-Aug-2026
**Lesson:** 12 of 63 — Phase 1: Foundations (Module 12 of 28)
**Module:** OAuth & Authentication
**Level:** Newbie → SDE2/SDE3 track | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: The student is a newbie. Teach every concept from first principles — technical and detailed, but explained so the student truly understands it and can apply it later in the System Design Track.

## Why This Module Matters
Every "Sign in with Google," every third-party app that reads your calendar, every mobile app that stays logged in for weeks — all of it runs on OAuth2 and tokens. Interviewers love this topic because it separates people who *understand* auth from those who confuse authentication with authorization and think a JWT is encrypted (it isn't). Get it wrong in production and you leak accounts: mishandled tokens and confused-deputy flows are behind a long list of real breaches. Today you'll learn the three server roles, the OAuth2 flows that matter, how a JWT is actually built and verified byte by byte, and the enduring sessions-vs-tokens trade-off that shapes every login system in Phase 2.

## Learning Objectives
By the end of this lesson you can:
- Distinguish authentication from authorization, and name the three OAuth2 server roles.
- Walk through the Authorization Code + PKCE flow step by step.
- Decode a JWT's three parts and explain exactly how the signature is verified.
- Explain why a JWT is signed but not (usually) encrypted, and what that means.
- Choose sessions vs tokens using statefulness, revocation, and scale as your criteria.

## The Lesson

### Auth Server, Authorization Server & Resource Server (the three roles)
**What it is (plain English):** OAuth2 separates responsibilities across roles. The **authorization server** authenticates the user and issues tokens. The **resource server** is the API holding the protected data (your photos, your email) and it *validates* tokens on each request. "Auth server" colloquially bundles authentication (proving *who* you are) with authorization (issuing tokens for *what* you may do); OpenID Connect adds the identity layer on top.

**The problem it solves:** You want a third-party app to access *some* of your data without handing it your password. Separating the token issuer from the data holder means the app never sees your credentials — it only gets a scoped, revocable token.

**How it works (mechanics):** Four parties interact:
```
[Resource Owner]  = you, the user
[Client]          = the third-party app (e.g., a photo printer)
[Authorization Server] = Google's login + consent + token endpoint
[Resource Server] = Google Photos API (holds the data)

Client → Auth Server: "let this user grant photo read?"
User   → Auth Server: logs in + consents
Auth Server → Client: access token (scope=photos.read, TTL=3600s)
Client → Resource Server: GET /photos  Authorization: Bearer <token>
Resource Server: validate token + scope → return photos
```
The token carries **scopes** (`photos.read`) and a short TTL (e.g., 1 hour). The resource server never asks the auth server per request if the token is a signed JWT — it just verifies the signature.

**Trade-offs / when NOT to use it:** Splitting roles adds moving parts and round trips; for a single first-party app with its own users, full OAuth is overkill — a plain session may suffice. Misconfigured redirect URIs or scope creep are common footguns.

**Where you'll see it:** Google, GitHub, Okta, and Auth0 all expose an authorization server + resource servers; "Sign in with X" is OpenID Connect over OAuth2.

### OAuth2 Flows
**What it is (plain English):** A "flow" (grant type) is the choreography by which a client obtains a token, tuned to the client's trust level. A server backend can keep a secret; a mobile app or SPA can't, so it needs a flow that's safe without one.

**The problem it solves:** Different clients have different security properties. Using the wrong flow — e.g., putting a client secret in a phone app anyone can decompile — leaks credentials. Flows match the ceremony to the client.

**How it works (mechanics):** The important ones:
- **Authorization Code (+ PKCE):** the gold standard. The client sends the user to the auth server; the user logs in and consents; the auth server redirects back with a short-lived **code**; the client exchanges the code (server-to-server) for tokens. PKCE adds a one-time `code_verifier`/`code_challenge` so an intercepted code is useless.
```
1 Client → /authorize?...&code_challenge=SHA256(v)
2 User logs in + consents
3 Auth Server → redirect ?code=XYZ
4 Client → /token  code=XYZ + code_verifier=v
5 Auth Server checks SHA256(v)==challenge → returns access+refresh tokens
```
- **Client Credentials:** no user — machine-to-machine. Service authenticates with its own ID/secret to get a token. Used for backend jobs.
- **Refresh Token:** a long-lived token used to silently get new short-lived access tokens, so the user isn't re-prompted hourly.
- **(Deprecated)** Implicit and Resource-Owner-Password flows — avoid; PKCE replaced Implicit for SPAs.

**Trade-offs / when NOT to use it:** Authorization Code has more round trips than Implicit but is far safer — always prefer it now. Refresh tokens are powerful and must be stored securely (theft = long-lived access), so rotate them.

**Where you'll see it:** "Sign in with Google/GitHub" uses Authorization Code + PKCE; internal service mesh auth uses Client Credentials.

### JWT (JSON Web Token)
**What it is (plain English):** A JWT is a compact, self-contained token: a JSON payload of claims (who, what scopes, expiry) that's **signed** so the recipient can verify it wasn't tampered with — without calling a database. It's three Base64URL parts joined by dots: `header.payload.signature`.

**The problem it solves:** Server-side sessions require a lookup on every request (find the session in a store). A signed JWT lets any service verify the token locally with a public key — no shared session store, no per-request DB hit. That's why it scales across microservices.

**How it works (mechanics):** Three parts:
```
header  = {"alg":"RS256","typ":"JWT"}
payload = {"sub":"user42","scope":"photos.read","exp":1786000000,"iat":1785996400}
signature = RS256_sign(base64(header) + "." + base64(payload), privateKey)
JWT = base64url(header).base64url(payload).base64url(signature)
```
Verification on the resource server: recompute the signature over `header.payload` using the auth server's **public** key; if it matches and `exp` hasn't passed, trust the claims. With RS256 the issuer signs with a private key and every service verifies with the public key — no secret sharing. A typical access-token JWT is ~300–800 bytes and verifies in microseconds.

**Trade-offs / when NOT to use it:** A JWT is **signed, not encrypted** — anyone can Base64-decode and read the payload, so never put secrets/PII in it. The big weakness is **revocation**: a JWT is valid until `exp`, so you can't instantly kill a stolen token without a blocklist (which reintroduces state). Keep TTLs short (5–15 min) and use refresh tokens.

**Where you'll see it:** Auth0, AWS Cognito, and most microservice auth use RS256 JWTs; OIDC `id_token` is a JWT.

### Sessions vs Tokens
**What it is (plain English):** Two ways to remember a logged-in user. A **session** stores state on the server (a session record) and hands the client an opaque **session ID** (usually in a cookie). A **token** (JWT) is stateless: the client holds a self-contained signed token and the server keeps nothing.

**The problem it solves:** HTTP is stateless, so after login you need to recognize the user on the next request without re-entering the password. Both solve that; they differ in *where the state lives*.

**How it works (mechanics):**
```
SESSION:  login → server stores {sess_abc → user42} → cookie: sess_abc
          each request → server looks up sess_abc in store → knows user42
          logout → delete the record → instantly revoked

TOKEN:    login → server signs JWT{sub:user42,exp} → client stores it
          each request → server verifies signature locally → knows user42
          logout → token stays valid until exp (needs blocklist to kill)
```
Numbers: a session lookup is a ~1 ms Redis hit per request but revocation is instant; a JWT verify is ~microseconds with *no* store but revocation is hard. At 100K QPS across 50 microservices, avoiding a shared session lookup on every hop is a real win — which is why stateless tokens dominate microservice architectures, while classic web apps often keep server sessions for easy revocation.

**Trade-offs / when NOT to use it:** Sessions need a shared, highly-available store (sticky sessions or Redis) and add a lookup per request, but give instant logout. Tokens scale statelessly but make revocation and logout genuinely hard. Many systems do **both**: short-lived JWT access tokens + a server-stored refresh token you can revoke.

**Where you'll see it:** Traditional Rails/Django apps use server sessions; SPAs and mobile + microservices use JWT access + refresh tokens; the hybrid is the modern default.

## Comparison Table

| Dimension | Server Session (opaque ID) | Token (JWT) |
|---|---|---|
| State location | Server store (stateful) | Client holds it (stateless) |
| Per-request cost | ~1 ms store lookup | µs local signature verify |
| Revocation / logout | Instant (delete record) | Hard (wait for exp / blocklist) |
| Scales across services | Needs shared store | No shared store needed |
| Readable by client | No (opaque) | Yes (Base64 — no secrets!) |
| Best for | Classic web apps, easy revocation | Microservices, mobile, SPAs |

**Verdict:** Use short-lived JWTs for stateless scale plus a revocable server-side refresh token — the hybrid gives you both scale and a logout that actually works.

## Common Misconceptions
- **Myth:** Authentication and authorization are the same. → **Reality:** Authentication proves *who* you are; authorization decides *what* you may do. OAuth2 is primarily authorization; OIDC adds authentication.
- **Myth:** A JWT is encrypted. → **Reality:** It's Base64-encoded and *signed*, not encrypted — anyone can read the payload, so never store secrets in it.
- **Myth:** OAuth means you share your password with the app. → **Reality:** The whole point is the app never sees your password — it gets a scoped, revocable token from the authorization server.
- **Myth:** JWTs can be revoked as easily as sessions. → **Reality:** A JWT is valid until it expires; instant revocation needs a blocklist, which reintroduces server state.
- **Myth:** The Implicit flow is fine for SPAs. → **Reality:** It's deprecated; use Authorization Code + PKCE, which is safe without a client secret.

## Real-World Case
The classic OAuth failure mode is the "confused deputy" and its cousin, the redirect-URI attack. In several documented incidents, apps registered loose redirect URIs (or accepted wildcard/subdomain redirects), letting an attacker craft an `/authorize` request that sent the authorization *code* to a URL they controlled — then exchanged it for the victim's tokens. The fix that became mandatory: exact-match redirect URIs plus **PKCE**, so even a stolen code is worthless without the matching `code_verifier` that never left the legitimate client. This is why modern guidance (OAuth 2.1) deprecates the Implicit flow entirely and requires PKCE for public clients. The lesson: in auth, the dangerous bugs aren't in the crypto — they're in the *flow choreography*, redirect handling, and token storage.

## Self-Test (answers at the bottom)
1. In one sentence, what's the difference between authentication and authorization?
2. Name the three OAuth2 server roles and what each one does.
3. Walk through the five steps of the Authorization Code + PKCE flow.
4. A teammate wants to store the user's email and a "isAdmin" secret flag inside a JWT payload for convenience. What do you tell them, and why?
5. Design sketch: Design login for a mobile banking app with 20M users across a microservices backend, where compromised-account logout must take effect within seconds. Sessions, tokens, or both? Specify token TTLs, refresh strategy, and how you achieve fast revocation.

## Interview Soundbites
- "Authentication is who you are; authorization is what you can do — OAuth2 is an authorization framework, and OpenID Connect adds the authentication layer."
- "A JWT is signed, not encrypted — the resource server verifies it locally with the issuer's public key, which is exactly why it scales across microservices without a shared session store."
- "Stateless tokens win on scale but lose on revocation, so I run short-lived JWT access tokens with a revocable server-side refresh token — the hybrid gives me both."

## Mini-Assignment
(~30 min) (1) Take a real JWT from jwt.io (or hand-write one): decode the three parts and label each claim (`sub`, `scope`, `exp`, `iat`), then explain in writing how the resource server verifies the signature with RS256. (2) Draw the Authorization Code + PKCE sequence for "Sign in with Google" on a mobile app, marking where the `code_challenge` and `code_verifier` are used and why an intercepted code is useless. (3) Write one paragraph choosing sessions vs tokens for that app and justify it on revocation and scale.

## Recap & Tomorrow
- **Three roles:** authorization server issues scoped tokens after authenticating the user; the resource server validates tokens and serves data; the client never sees the password.
- **OAuth2 flows:** Authorization Code + PKCE is the default; Client Credentials for machine-to-machine; refresh tokens for silent renewal; avoid Implicit/Password.
- **JWT:** `header.payload.signature`, signed (not encrypted), verified locally with a public key; short TTLs because revocation is hard.
- **Sessions vs tokens:** stateful session (instant revoke, per-request lookup) vs stateless token (scales, hard to revoke); the modern default is a hybrid.

Tomorrow we close Phase 1's networking-and-security arc and move toward the data-and-storage core — **Lesson 13**, where we start turning these foundations into full end-to-end system designs.

## Self-Test Answers
1. Authentication verifies *who* the user is (proving identity, e.g., via password/login); authorization decides *what* that authenticated user is allowed to do (which scopes/resources they may access).
2. The **authorization server** authenticates the user, handles consent, and issues access/refresh tokens; the **resource server** holds the protected data and validates the token (and its scopes) on each request; colloquially the "auth server" also covers authentication/identity, which OpenID Connect formalizes.
3. (1) Client redirects the user to the auth server's `/authorize` with a `code_challenge = SHA256(verifier)`. (2) The user logs in and consents. (3) The auth server redirects back with a short-lived authorization `code`. (4) The client calls `/token` with the code plus the original `code_verifier`. (5) The auth server checks `SHA256(verifier) == code_challenge`, and if it matches returns the access (and refresh) tokens.
4. Don't. A JWT is only Base64-encoded and signed, not encrypted, so anyone holding the token can decode and read the payload — the email is PII exposure and the `isAdmin` secret would be visible and, worse, is a security flag that must be authoritative server-side. Keep secrets/PII out of the JWT; store authorization decisions server-side or in short-lived, minimal claims.
5. Use **both** (hybrid). Issue short-lived JWT access tokens (TTL 5–15 min) for stateless verification across microservices, plus a long-lived, server-stored **refresh token** the user exchanges for new access tokens. Fast revocation: on compromise, revoke the refresh token immediately (delete/blocklist it server-side) so no new access tokens can be minted, and maintain a short access-token blocklist checked at the gateway so existing tokens die within the ~5-15 min TTL — or push a revocation event to services. The short TTL bounds exposure while the revocable refresh token gives near-instant logout.

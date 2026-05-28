---
category: technique
title: "Business Logic Flaws - Abusing Intended Functionality"
tags: [business-logic, workflow-bypass, price-manipulation, race-condition, quantity, coupon, replay, state-machine, quota-bypass, logic-flaw, high-value]
---

# Business Logic Flaws

These are the bugs scanners cannot find and the bugs that pay the most in mature applications. There is no malformed input, no injection payload - every request is individually valid. The flaw is that the sequence, the values, or the timing violate an assumption the developer never enforced.

You cannot grep for a business logic bug. You have to understand what the application is supposed to do, then ask: what happens if I do it wrong, out of order, too fast, or with values nobody expected?

This is where the "незначительный косяк с серьёзными последствиями" lives - a single missing check in an otherwise hardened app.

---

## The method: model the intended flow, then break each assumption

For any feature, write down the happy path as the developer imagined it:
```
add to cart -> apply discount -> enter payment -> charge -> confirm order -> ship
```
Then attack each transition and each value with these questions:
- Can I skip a step? (state machine bypass)
- Can I repeat a step? (replay)
- Can I do steps out of order?
- Can I run two steps at the same time? (race)
- What values did they assume but not enforce? (negative, zero, huge, wrong currency)
- Who is allowed to do this, and did they check at THIS step or only an earlier one?

---

## State machine / workflow bypass

Multi-step flows assume you completed prior steps. Most validate the final step's input but not that you legitimately reached it.

```http
# Checkout assumes payment happened. Call the confirm/ship step directly:
POST /api/order/confirm  {"order_id": 1337}      # without ever paying

# Password reset: 1) request 2) verify code 3) set password
# Jump to step 3 directly - is the code actually required, or just the UI gate?
POST /api/reset/set-password  {"user":"victim","new_password":"x"}

# Multi-step KYC / approval - submit the final "approved" state directly
PATCH /api/application/55  {"status":"approved"}
```

Whitebox: look for handlers that read a target state from the request instead of computing the next valid transition server-side. `status = request.status` is a red flag; `status = next_state(current)` is safe.

---

## Value manipulation - the assumptions in numbers

Developers validate type and presence, rarely the full domain of a value.

```http
# Negative quantity -> negative total -> refund / balance increase
POST /api/cart  {"item":"x","quantity":-5}

# Zero or fractional price tier
POST /api/order  {"item":"premium","price":0}
POST /api/order  {"amount":0.001}            # rounds to 0 charged, value delivered

# Integer overflow on quantity -> wraps total to a small/negative number
{"quantity": 4294967296}

# Currency confusion - pay in a weaker currency the backend treats 1:1
{"amount":100,"currency":"IDR"}   # charged as if 100 USD-equivalent? or 100 IDR?

# Decimal/rounding abuse - many tiny transactions where rounding favors you
```

Always test: negative, zero, very large, fractional, and a different unit/currency than the UI offers.

---

## Race conditions on logic (not just data)

When a check and the action it guards are not atomic, fire concurrent requests in the TOCTOU window. Deep dive in async-pipeline-race; the business-logic targets:

```
- Coupon "single use": redeem the same coupon in 20 parallel requests -> applied N times
- Gift card / balance: spend the same balance concurrently -> spend more than you have
- Withdrawal / transfer limit: 10x parallel withdrawals each under the limit -> exceed it
- "One vote / one claim per user": claim concurrently -> claim many
- Invite / referral bonus: redeem one invite many times in parallel
```

```bash
# Built-in: fire concurrent identical requests
make_race_requests(method: POST, url: /api/coupon/redeem, body: '{"code":"SAVE50"}', count: 30, threads: 15)
```

The tell: any "you can only do this once" or "you have N remaining" check that reads-then-writes without a lock or atomic decrement.

---

## Replay and idempotency

```
- Replay a "payment succeeded" webhook -> credited twice (see api-security webhooks)
- Replay a signed request whose nonce is not tracked -> repeat a one-time action
- Resubmit a discount/cashback claim -> stacks
- Re-use a consumed OTP / reset token -> was it actually invalidated after use?
```

Test every "one-time" action twice. The second attempt should fail; if it succeeds, idempotency is broken.

---

## Discount, coupon, and incentive abuse

Promotions are business logic under attacker-chosen inputs - a rich target:
```
- Stack coupons not meant to combine (apply two, three, the same one twice)
- Apply a coupon, then remove the item it required but keep the discount
- Negative-priced item to offset a minimum-spend threshold
- Referral self-loop: refer yourself with a second account, collect both bonuses
- Price-match / cashback computed on pre-discount price -> get paid to buy
- Cancel-after-benefit: claim the signup credit, refund the purchase, keep the credit
```

---

## Quota, limit, and tier bypass

```
- Free-tier limit enforced in the UI/client only -> call the API past the limit
- "10 per month" counted per calendar field you control in the request
- Trial extension by resetting an account attribute (created_at, trial_ends)
- Resource limits checked at request time but not re-checked on async completion
```

---

## Authorization at the wrong step

A subtle, high-value class: the permission is checked early but the sensitive action happens later, after state the attacker can change.

```
- Cart ownership checked when items added, but checkout reads cart_id from the request
  -> check out someone else's cart
- File access authorized at upload, served later by a path that skips the check
- Role cached in session at login; role downgraded server-side but session still elevated
  until re-auth -> act with stale elevated role
```

Map: where is the authorization decision made, and where is the protected action performed? If state can change between them, or the action reads an id the check did not bind to, it is exploitable. (This connects to assumption-hunting and trust-graph - the gap between an early check and a late action.)

---

## Why this matters most on mature targets

A bank, a lab system, a fintech - they will have fixed the SQLi and the XSS. What survives is the one workflow where a developer assumed "the user would never send a negative amount" or "you obviously cannot confirm an order you did not pay for." That single unenforced assumption, in an otherwise hardened system, is the high-severity finding. You find it by understanding the business, not by fuzzing.

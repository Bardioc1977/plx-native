# Companion (being a remote-control target)

Spec for letting a Plex app (phone, tablet, Plex Web) pick this TV as a playback target and drive it:
"Play on TV", pause, seek, skip, change tracks. Status: **proposed, nothing built, and the key wire
facts are still UNVERIFIED** (§3, §9). Fourth of the Plex-server parity specs
(`docs/parity-specs.md`).

**Scope: remote control of video playback on this app, through a PMS the account can reach.** Not
casting TO other players, not music or photo control, not Plex Home theatre "mirror" screens.

## 1. The gap

The app announces itself as a player on every playback request (`X-Plex-Provides: player`,
`plex/src/plex/client.rs`, `plex/src/plex/account.rs`), keeps a stable client identifier, and reports
a timeline (`POST /:/timeline`) into a `continuous=1` play queue, so it shows up in Now Playing. But
nothing can drive it:

- there is no `/player/*` handler, no registration with the server, and no timeline answer for a
  controller (`docs/parity-gaps.md` lists "no Companion receiver" twice, as "awkward" and "large");
- `plex/src/plex/timeline.rs` calls the session "remote-controllable", which overstates what is
  built;
- the notification feed deliberately omits `X-Plex-Provides` because "this client is not a
  controllable player" (`plex/src/plex/notify.rs`).

The earlier assumption (in `parity-gaps.md`) was that Companion needs an HTTP **server** in the
app, GDM multicast on UDP 32412/32414, and a listener on a fixed port. On a 32-bit TV whose
networking is client-only (`stream.rs`, `curlio.rs`), that made it look like the most expensive gap
on the list. §3 shows why it may not be.

## 2. Principles

1. **Outbound only, if the server allows it.** Prefer the path where the TV connects to the PMS and
   never listens. That keeps the sandbox, the firewall and the 60 MB memory declaration out of the
   discussion, and it works off-LAN.
2. **A controller never gets to point the app somewhere.** Commands carry `address`, `port`, `token`
   and `machineIdentifier` fields (as third-party client libraries describe `playMedia`). The app
   resolves the target ONLY through its own server registry (`plex::client_for`,
   `plex/src/plex/CLAUDE.md`, *granted, pinned, reachable*); a command naming an unknown machine is
   refused, and a token or address from a command is never used or logged. Otherwise a crafted
   command could make the TV send its credentials to an arbitrary host.
3. **Commands reuse the transport that exists.** Pause, resume, seek, stop, next, previous and
   track choice already have entry points the remote keys reach (`request_seek`, `pause`, `resume`,
   the track commits in `route/`). Companion produces the same typed requests, delivered to the main
   thread through the frame loop's mailbox, never from the poll worker (the `MainThread` token in
   `player/mod.rs` forbids it).
4. **Logic performs no IO** (`docs/sans-io.md`). A `CompanionMachine` owns the state (registered or
   not, last command id, subscription) and emits typed effects (`Poll`, `Reply`, `Unsubscribe`); the
   application's runners execute them and every result returns as one addressed event.
5. **Opt-in on a household's TV.** A Settings switch, off until the feature is device-verified, and
   a plain status line. Being controllable is something the owner chooses.

## 3. What is known, and what is not

### Measured on the owner's PMS (1.43.5.11029, 2026-10-24; GET and log reads only, ids and addresses redacted)

- `GET /clients` answers an empty list while no player is registered. The server has no GDM lines in
  its log.
- The server's own log contains a subsystem named **`CompanionProxy`**. Other clients on the
  account have, repeatedly, made these requests to the PMS itself:
  - `GET /player/proxy/poll?deviceClass=pc&protocolCapabilities=playback&protocolVersion=1`
    carrying the usual `X-Plex-*` identity headers and a signed-in token. The server logs
    `registered new player: <client id> (<ip>)` for the first poll and answers later polls
    **204 No Content** after about 30 seconds (a long poll that timed out with nothing to deliver).
  - `GET /player/timeline/subscribe?…&protocol=…` answered **400**, logged as "wanted to subscribe
    with an unsupported protocol '…'": in the captured requests the `protocol` value was a URL, not
    a scheme, which the server refuses.
  - `GET /player/timeline/unsubscribe?…` answered **200**.
- No controller command (`X-Plex-Target-Client-Identifier`, `/player/playback/*`) appears in the
  available log window, so the command side is not observed.

That is the whole evidence. It shows the **server-proxied Companion** exists on 1.43.5: a player
that cannot accept inbound connections registers by long-polling the PMS, and the PMS holds
controller commands for it. Third-party client libraries describe the controller half
(`X-Plex-Target-Client-Identifier` plus `/player/playback/{playMedia,pause,play,stop,seekTo,
skipNext,skipPrevious,stepForward,stepBack,setParameters,setStreams,skipTo}` and
`/player/navigation/*`); the app has not verified any of that against this server.

### UNVERIFIED and decisive

1. **What a poll that has a command returns** (status, body, how the command's path and query are
   encoded), and what the player must send back (a `<Response code="200" status="OK"/>`-style
   answer, to where).
2. How a player answers a controller's **timeline subscribe/poll** in proxy mode (what is sent,
   how often, the `commandID` handshake).
3. Whether a poll-registered player **appears in the Plex mobile apps' "play on" list** and in
   `GET /clients`. If the apps list only players that publish a reachable address through plex.tv,
   the proxy path is not enough and the direct listener (§8) comes back on the table.
4. The accepted `protocol` values for subscribe and whether the proxy flow needs a subscribe at all.
5. Whether commands to a proxy player work for a controller on a different network (the point of
   the proxy).

Everything below that depends on these is marked as such. Phase 0 exists to close them.

## 4. Behaviour

### 4.1 Registration and polling

- When Companion is on, the app has a signed-in profile and a reachable owned or shared server
  (`granted`, `reachable`), a **poll worker** per server issues the long poll and restarts it as soon
  as it returns, with the existing backoff on errors. A 204 is the normal idle answer.
- Registration identity is the app's persisted client identifier and device name, exactly the headers
  every request already carries (`Client::headers`); no new identifier is minted (a new identifier
  raises a new-device alert on the account — `plex/src/plex/CLAUDE.md`, notify bullet).
- The poll runs only while the app is in the foreground lifecycle and is cancelled on suspend, exit
  and sign-out (it unsubscribes first). A poll must be cancellable well inside the app's shutdown
  budget; the long timeout never blocks teardown.
- Servers: the primary server and each shared server the profile may use poll independently, since
  the controller addresses a server. Phase 1 may start with the current server only (decision in
  §7).

### 4.2 Commands (phase 1)

The machine parses a delivered command into a typed value, validates it (§2 rule 2), and emits a
request the main thread executes:

| command | effect |
|---|---|
| `playMedia` | start playback of the controller's play queue at `key`/`offset`, on the named server from the registry (§4.3) |
| `pause`, `play`, `stop` | the existing transport; `stop` ends playback and reports `stopped` |
| `seekTo` (ms) | `request_seek` |
| `stepForward`, `stepBack`, `skipNext`, `skipPrevious` | step by the configured skip interval; next/previous move within the play queue |
| `setStreams` | audio and subtitle choice through the existing track commits, so the server selection and track memory behave as for a viewer's pick |
| `setParameters` | volume and similar: acknowledged and ignored (the TV's volume is not the app's) |

Every command gets the acknowledgement the protocol requires (shape UNVERIFIED, §3.1). An
unsupported or malformed command is answered with an error status and logged by name only.

### 4.3 `playMedia` and the controller's queue

The controller creates a play queue on the server and sends its id (`containerKey`) and the item
`key`. Today `resolve_playqueue` (`media/src/route/decision.rs`) creates its own queue. Companion
needs the other direction: **adopt a queue created elsewhere** — fetch it
(`GET /playQueues/{id}`), keep its window as `QueueRow`s, start `key`, and report the timeline
against the controller's `playQueueID` so the controller's UI shows the right state. This is the
same machinery `docs/playlists-spec.md` §4.3 needs for window refill, and a playlist or a show
started from a phone arrives here as an ordinary queue. The version and track rules of
`docs/version-selection-spec.md` apply to each entry unchanged. The resume point is the
controller's `offset`, else the item's own.

The decision to start playback is the app's: it goes through `request_play`, so direct play, the
quality ladder, the source gate and the error read-outs behave as for a local start.

### 4.4 Timeline answers

While a controller is subscribed (or polling), the app answers with the current state of the
playing item (type, state, time, duration, ratingKey, key, containerKey, machineIdentifier,
volume) — the same facts the `/:/timeline` reporter already samples
(`player/threads.rs`, `TimelineState`). It adds `paused`/`buffering` states and an immediate answer
on change; today's reporter posts every 10 seconds, which a live remote cannot use (the playback
audit lists this as a partial gap). With no media playing the timeline says `stopped`/navigation.

### 4.5 Navigation commands (phase 2)

`/player/navigation/{moveUp,moveDown,moveLeft,moveRight,select,back,home,…}` map to the same key
events the Magic Remote produces, through the one key router (`app/input.rs`, `FocusEngine`). The
mapping lives in one table. `home`/`back` obey the existing BACK trail. Navigation while a
modal alert is open is refused with an error status. Text entry (`setText`) is out of scope until the
search keyboard seam is understood for remote input.

### 4.6 What the viewer sees

- Settings > Playback (Install scope, default off): **Allow remote control**, with a one-line
  explanation and a status value ("Ready", "Not reachable", "Off"). The row is a new `setting!` per
  the contract in `plex/src/plex/CLAUDE.md`.
- A toast when a controller starts playback ("Playing from <device name>"), if the command carried
  a name; nothing otherwise. No on-screen chrome while idle.
- The Diagnostics panel gains one line (registered, last poll result, last command name).

## 5. Layering and files

- `plex`: pure wire code, no sockets of its own: poll request and response parsing, command parsing
  and validation, timeline answer encoding. Uses `http.rs` for the long poll with a dedicated
  timeout class.
- `data` or `appkit`: `CompanionMachine` and its effects/events; `ci/allow/workers` gains no line —
  the poll worker is started by an application runner.
- `media`: queue adoption, immediate state reporting.
- `screens`: the Settings row and status text.
- `app`: the runners, the mailbox drained in the frame loop, the key-event mapping.
- No `webos`/`port` name outside `[port webos]`. `ci/module-layers.ini` lists the new modules.

## 6. Phases

**Phase 0 — probes** (§9). Nothing ships until §3's unknowns 1, 2 and 3 are settled and written into
this file.

**Phase 1 — playback control.** Registration/polling, `playMedia` with queue adoption, pause/play/
stop/seek/step/skip, `setStreams`, timeline answers, the Settings switch and status, mock support and
tests. Acceptance on the mock: a scripted controller starts a 3-entry queue, pauses, seeks, skips,
switches audio and stops; the app's state follows, a command naming an unknown machine is refused,
and the poll worker is gone within the shutdown budget.

**Phase 2 — navigation.** The `/player/navigation/*` table and refusal rules.

**Phase 3 — direct listener (only if probe 3 says the proxy is not enough).** A small inbound HTTP
listener plus GDM answers (UDP 32412/32414), advertising through plex.tv. This is the "large" item
the old audit meant. It needs its own spec: the sandbox and firewall behaviour on webOS 4.5 must be
measured first (`webOS native behavior is under-documented`, AGENTS.md), the listener must be
LAN-only, and a connection from outside the household's networks is refused.

## 7. Decisions to take

1. **Default**: off in phase 1 (recommended), on once device-verified?
2. **Servers**: poll the current server only, or every granted server?
3. **Foreground only** (recommended) or also while the app is in the background on the set?
4. Whether a controller may start playback while another title plays (replace, as the official
   clients do) or must wait.

## 8. Alternatives considered

- **Direct listener + GDM from the start**: works for LAN-only controllers and does not depend on the
  proxy, but is new inbound-network code on a device whose sandbox is unmeasured, and the old audit
  priced it "large". Kept as phase 3.
- **Doing nothing and relying on Plex Web's "cast"**: not available; the app is not discoverable.

## 9. Verification and the real server

Host (`make check`; `make test-crate C=plx_plex|plx_data|plx_media`; `make test-hostsim`): command
parsing for every command and a malformed one, the validation rule (unknown `machineIdentifier`,
foreign `address`/`token`, never used, never logged), the machine's states and effects (register,
idle 204, command, error backoff, cancel, sign-out), queue adoption (window, `key`, `offset`),
timeline answer encoding, the immediate-state reporter, Settings round trip and the account-bound
clear, shutdown within budget.

Mock: `tests/mock_pms.py` grows `/player/proxy/poll` (long poll with a scripted queue of commands),
`/player/timeline/*`, and a scriptable controller; the recorded command sequences are replay
fixtures built from the closed alphabet (`tests/replay_fixtures.py`), never from a household
response. The shape of a delivered command comes from probe 1, not from this document.

Device (`tv-lock` first; `which-tier`): the sequence above on the set against the mock, with
`--guest`. Frame pacing while a poll is outstanding is measured with the `profile-tv` skill before
the feature is defaulted on.

**The real server.** Reading is allowed (logs, `GET /clients`). Registering a probe player and
sending it a command writes transient state on the PMS, which the owner has not allowed, so probes 1
to 5 need the owner's explicit OK for that session, or a scratch PMS that holds no household data
(for example a throwaway container on the NAS, which also needs approval). The probe tool is a
small script that acts as a poller and as a controller; it prints shapes only.

## 10. Out of scope

Casting to other players; audio and photo control; mirror/"now playing" screens; remote text entry;
volume control; multi-room sync; controlling the app while signed out.

## 11. Prose this change makes false (for `doc-claim-auditor`)

`docs/parity-gaps.md` (the three Companion entries), `plex/src/plex/timeline.rs` ("remote-controllable"
claim becomes true or is reworded), `plex/src/plex/notify.rs` (the "not a controllable player"
sentence about `X-Plex-Provides`), `docs/pms-api.md` (the `X-Plex-Provides` row and a new
Companion section), and `docs/playback-settings.md` for the new row.

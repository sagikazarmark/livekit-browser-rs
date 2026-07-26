# Demo Application

The repository ships a Dioxus full-stack demo that exercises every workflow this wrapper
supports, against a local LiveKit server or a LiveKit Cloud project. It is a development
harness, not a deployable application.

## Run it

Requirements: Docker Engine and Docker Compose v2. No host Rust, Node.js, or Dioxus installation is needed.

Every command in this document runs from the repository root.

```sh
./run-prototype.sh
```

The first run downloads the pinned toolchains and builds the Dioxus client and server bundles, so it takes longer than subsequent runs. The command prints two URLs:

- Desktop: `http://localhost:8081`
- Mobile: an HTTPS URL such as `https://192.168.1.42.nip.io:8443`

In the browser:

1. Inspect the permission-free capability panel and follow the guided lifecycle checklist.
2. Keep or change the room name and unique Participant Identity, then use **Open second participant** to prefill the same room in another tab.
3. Select **Refresh input devices** to request labels, then choose a camera, microphone, and camera facing mode.
4. Select **Start camera** before or after joining to open a private local preview.
5. Select **Start microphone** before or after joining to capture audio independently of the camera.
6. Select **Start screen share** to choose a display and optionally request system or tab audio.
7. Select **Join room** in each tab and confirm joining itself does not capture or publish media.
8. Exchange visible typed Chat Messages independently from Room Events.
9. Restart or switch either active hardware input without rebuilding the Room Session or Track Publication.
10. Publish camera, microphone, display video, or available system audio independently, then unpublish without stopping capture.
11. Create a bounded Local Clip, inspect its browser-owned artifact details, then download or revoke its object URL.
12. Allow all participants, deny all participants, or grant one Participant Identity access to current and future local Track Publications.
13. Update Local Participant metadata or patch and remove participant attributes.
14. Stop camera, microphone, or the complete screen-capture set to release those browser resources.
15. Select **Leave room** and confirm deterministic disconnection.

System audio remains optional even when requested: browsers and selectable display sources differ in what they provide. Display chooser cancellation and policy denial share the browser's `NotAllowedError` and are intentionally reported as one typed outcome. Display capture must be started directly from an active, focused user interaction.

Local Participant metadata updates must be non-empty because the pinned LiveKit server treats an empty metadata string as no update. Attribute patches use LiveKit's empty-value deletion semantics.

The browser receives only the signaling URL and a participant token issued by the example's Dioxus server. The LiveKit API secret remains server-side.

## Try It With LiveKit Cloud

The same example can use a [LiveKit Cloud](https://cloud.livekit.io/) project instead of the local LiveKit server. Create a project, then copy its WebSocket URL, API key, and API secret from the project settings.

Create a `.env` file in the repository root with those values:

```dotenv
LIVEKIT_URL=wss://your-project.livekit.cloud
LIVEKIT_API_KEY=your-api-key
LIVEKIT_API_SECRET=your-api-secret
```

Use the exact WebSocket URL shown by LiveKit Cloud. Keep the `wss://` scheme and do not add `/rtc`.

Start the prototype normally:

```sh
./run-prototype.sh
```

The example's Dioxus server signs participant tokens with the Cloud credentials and returns the Cloud URL to the browser. The API secret stays in the full-stack app container and is never sent to the browser. The local LiveKit container is not started while `LIVEKIT_URL` is set, and a LAN address is only needed if you want to open the app from another device.

Without a LAN address, the HTTPS endpoint is `https://localhost:8443`. Trust `.prototype-runtime/caddy-local-root.crt` before using it. Plain `http://localhost:8081` remains a browser secure context without a certificate because browsers treat `localhost` specially.

For Firefox, open **Settings > Privacy & Security > Certificates > View Certificates > Authorities > Import**, select `.prototype-runtime/caddy-local-root.crt`, and trust it to identify websites. Then reopen `https://localhost:8443`.

The repository ignores the root `.env` file, but it still contains a credential that can authorize access to the Cloud project. Do not share it, commit it under another name, or use this unauthenticated example token endpoint in production. To switch back to the local development server, remove the three values from `.env` and rerun `./run-prototype.sh`.

## Test From A Phone

Browsers only expose camera and microphone APIs from secure contexts. Plain HTTP works for the special `localhost` origin, but not for a LAN IP, so the stack includes a local HTTPS certificate authority.

If the printed address is not the computer's Wi-Fi or Ethernet address, restart with it explicitly:

```sh
LAN_IP=192.168.1.42 ./run-prototype.sh
```

The script writes the local root certificate to:

```text
.prototype-runtime/caddy-local-root.crt
```

Transfer that file to the phone and trust it before opening the printed mobile URL.

### iPhone or iPad

1. Open the certificate file on the device and allow the profile download.
2. Open **Settings > General > VPN & Device Management** and install the profile.
3. Open **Settings > General > About > Certificate Trust Settings** and enable full trust for the Caddy local authority.
4. Open the printed `https://<LAN-IP>.nip.io:8443` URL in Safari.

### Android

1. Open **Settings > Security & privacy > More security settings > Install a certificate > CA certificate**. Labels vary by vendor.
2. Select `caddy-local-root.crt` and confirm the warning for this temporary development CA.
3. Open the printed `https://<LAN-IP>.nip.io:8443` URL in Chrome.

Managed phones may prohibit user-installed CAs. Remove the CA after testing if it is no longer needed.

The `nip.io` hostname resolves its embedded address back to the LAN IP and gives TLS clients a hostname for SNI. If the network blocks public DNS wildcard services, provide a hostname that your phone resolves to this computer:

```sh
LAN_IP=192.168.1.42 LAN_HOST=livekit-prototype.example.test ./run-prototype.sh
```

## Network Ports

Allow these inbound ports in the host firewall and keep both devices on the same network:

| Port | Protocol | Purpose |
| --- | --- | --- |
| `8081` | TCP | Desktop HTTP app and signaling |
| `8443` | TCP | Mobile HTTPS app and secure signaling |
| `7881` | TCP | LiveKit ICE/TCP fallback |
| `7882` | UDP | LiveKit WebRTC media |

Port `7880/tcp` is also exposed for direct LiveKit diagnostics, but browser signaling normally goes through the same-origin gateway. Guest Wi-Fi client isolation, a VPN, or Docker Desktop without working UDP forwarding can prevent media even when the page itself loads.

When using LiveKit Cloud, only the app ports (`8081/tcp` or `8443/tcp`) need to be reachable on the development computer. Signaling and media use outbound browser connections to LiveKit Cloud rather than the local `7880`-`7882` ports.

If another process already uses the desktop HTTP port, select a different loopback port:

```sh
HTTP_PORT=8090 ./run-prototype.sh
```

## Automated Check

Install the pinned browser SDK, build both Dioxus app bundles, start the local stack,
and run the browser workflow with one command:

```sh
./test-prototype.sh
```

The required compatibility gate always uses the local LiveKit container, even when the root `.env`
configures LiveKit Cloud. This keeps the gate independent of Cloud quotas, service state, and network
availability. To run the same workflows against the configured Cloud project explicitly, use
`./test-prototype.sh cloud`. Cloud mode requires `LIVEKIT_URL`, `LIVEKIT_API_KEY`, and
`LIVEKIT_API_SECRET` and is an optional integration run.

This is the complete compatibility gate. The image build typechecks the TypeScript adapter, verifies
its committed JavaScript bundle, runs the adapter contracts and workspace tests, and runs host and
WASM all-feature typechecks. Playwright then runs the supported desktop
browsers, mobile-sized Chromium and WebKit consumers, lifecycle cleanup contracts, and the
near-100-participant snapshot profile against freshly recreated local stack containers. Named
volumes are retained so the trusted mobile CA remains stable.

With the local stack already running, rerun only the browser workflow with
`LIVEKIT_API_KEY=devkey LIVEKIT_API_SECRET=secret docker compose --profile test run --rm smoke`.

The Playwright check runs the Room Session contract in Chromium, Firefox, and WebKit and verifies that:

- Dioxus WASM mounts and reports browser support.
- A subtree provider reacts to Room Snapshots, exposes typed actions, and drops only its own Room Session clones when unmounted.
- Unstyled Dioxus local and remote media nodes attach and detach without changing capture, publication, or subscription state.
- The browser obtains a Rust-generated token and connects through Caddy to LiveKit.
- Joining requests no camera or microphone capture and publishes no tracks.
- Camera capture and local preview work before or after joining without implicit publication.
- Typed camera source, name, codec, simulcast, and sender encoding options reach LiveKit publication.
- Muting preserves the Track Publication, while unpublishing preserves capture and preview.
- Explicit stop and final-owner drop release capture and terminate active attachments.
- Microphone capture works before or after joining without implicit publication.
- Camera and microphone devices enumerate with permission-gated labels and typed capture constraints.
- Active inputs can restart or switch without replacing their Room Session or Track Publication.
- Permission denied, unavailable-device, and device-in-use failures are distinct and roll back coherently.
- Typed microphone source, name, fixed Opus codec policy, DTX, and RED policy cover LiveKit publication.
- Microphone mute preserves the Track Publication, unpublish preserves capture, and stop releases capture.
- Same-track audio mutations are serialized without blocking independent camera operations.
- Display capture returns required screen video and optional system audio without implicit publication.
- Screen video and system audio publish and unpublish independently with authoritative LiveKit sources.
- Browser stop-sharing and explicit aggregate stop terminate capture and local preview coherently.
- Display cancellation, missing capability, abandoned futures, and final-owner cleanup leave no partial media state.
- The initial snapshot contains authoritative Local Participant state and a monotonic revision.
- Two participants discover Participant Identity-keyed Remote Participant state, audio and video Track Publications, and separately modeled Remote Data Tracks.
- Remote Track Publications remain observable when muted, unsubscribed, or locally unavailable.
- Automatic and selective subscription resolve typed Remote Audio and Video Track handles.
- Publisher permissions allow all participants or identity-specific access without stopping capture or removing Track Publications.
- LiveKit publisher grants remain authoritative and denied mutations return typed permission failures.
- Room, Local Participant, and Remote Participant metadata and attributes remain coherent revisioned snapshot state.
- Permitted Local Participant metadata and attribute updates propagate to all participants, while denied updates remain typed and non-optimistic.
- Remote attachment close, drop, end, and replacement cleanup never mutate subscription policy.
- Browser-blocked remote audio is observable and retryable from a user activation.
- Local and Remote Participant connection quality stays coherent across revision-tagged snapshots and events.
- Published local media and subscribed remote media return populated typed sender and receiver statistics in every gating browser.
- Local and Remote Media Tracks carrying audio return owned level and waveform frames from live fake audio, while Audio Analyser close, final drop, source end, and replacement release their Web Audio graphs.
- Same-turn remote callbacks install one coherent snapshot revision before Room Events.
- Automatic reconnection remains within the original Room Session and browser Room object.
- Replaced Remote Participant, Track Publication, and Media Track objects terminate old handles and attachments even when logical IDs are reused.
- Lagging Room Event subscribers recover from the current snapshot without affecting independent subscribers.
- Multi-megabyte text and byte streams preserve native payloads, hold each sender write until the destination pulls it, complete explicitly, and terminate on cancellation or remote departure.
- Participant RPC preserves native text and byte responses, reports timeout and remote application failures distinctly, removes dropped methods, and cancels in-progress handlers on Room Session leave without producing Room Events.
- Chat Messages preserve native string content, ordering, sender identity, message IDs, and timing through bounded delivery that closes on Room Session leave without producing Room Events.
- Transcription Segments preserve source identity, Track Publication SID, stable segment identity, timing, language, and interim-to-final order through bounded delivery that ignores ended sources and closes on Room Session leave without producing Room Events.
- Dropping a Rust operation future leaves the JavaScript Promise running and its eventual state observable.
- Explicit leave removes listeners and disconnects deterministically.
- Final Room Session handle drop schedules best-effort disconnect.
- The controls fit a `390 x 844` mobile viewport without horizontal overflow.

See [Browser Compatibility](browser-compatibility.md) for UMD setup and load order, the
browser capability matrix, user-activation and cleanup rules, unsupported workflows, profile
methodology and results, and the atomic SDK upgrade procedure.


## Stop And Reset

Stop containers while retaining the local CA, so the phone trust remains valid on the next run:

```sh
docker compose down
```

Remove containers and the Caddy CA volume completely:

```sh
docker compose down --volumes
```

After removing the volume, delete the old CA from test devices because the next run creates a different authority.

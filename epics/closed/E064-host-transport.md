---
id: "E064"
title: Host transport integration
created: 2026-04-12
tickets: ["0348", "0349", "0350"]
adr: "0031"
---

## Summary

Enable the audio engine to receive and expose host transport state
(play/stop, tempo, beat/bar position, time signature) from a DAW via
the CLAP plugin, and allow the tracker sequencer to synchronise with
it.

The existing `GLOBAL_CLOCK` backplane slot is upgraded to a poly
`GLOBAL_TRANSPORT` carrying both the sample counter and host transport
lanes. The CLAP plugin populates the transport lanes from
`clap_event_transport`. A `HostTransport` convenience module unpacks
the lanes into named outputs for generative/unsequenced use.
`MasterSequencer` gains a `sync` parameter to read the backplane
directly in `host` mode.

See ADR 0031 for design rationale and alternatives considered.

## Tickets

| Ticket | Title                                          |
|--------|------------------------------------------------|
| 0348   | Upgrade GLOBAL_CLOCK to GLOBAL_TRANSPORT poly  |
| 0349   | HostTransport module                           |
| 0350   | MasterSequencer host sync mode                 |

0348 is the foundation; 0349 and 0350 depend on it but are
independent of each other.

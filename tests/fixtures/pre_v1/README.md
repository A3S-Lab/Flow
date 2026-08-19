# Retained pre-v1 histories

These fixtures are serialized by the named, immutable Flow release rather than
by the current source tree. They are release evidence, not examples that may be
rewritten when the model changes.

| Fixture | Producer tag | Producer commit |
| --- | --- | --- |
| `v0.5.0-running-step.json` | `v0.5.0` | `67888763589d2799bc456df21744d73ef8647d6a` |
| `v0.13.1-running-step.json` | `v0.13.1` | `c681a26cb89ffe22227db6bf49efc65c0f1fe83d` |

The first fixture establishes the supported durable-history floor. The second
captures the final published pre-v1 format, including a pinned runtime build.
Both histories stop after a step attempt starts so the v1 candidate must do
more than deserialize or inspect them: it must replay the original command,
redeliver the interrupted step, and reach a durable terminal state.

Do not regenerate a fixture with a newer Flow version. A new compatibility
boundary must use a new file whose producer tag and full commit are recorded
here.

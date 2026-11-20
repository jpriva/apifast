# apifast-script-js

**Layer:** Adapter

JavaScript engine of apifast, on QuickJS through rquickjs. Each script runs in a runtime of its own, made when it starts and dropped when it ends, with a time limit (waits included), a memory limit and a bounded native stack. A script has no modules: it reaches neither the file system nor the network, only the globals the engine gives it: `console` and Bruno's API (`req`, `res`, `bru`, `test`, `expect`), written in JavaScript over the data the engine is given (`ScriptInput`); what the script changes comes back as data (`ScriptOutcome`). It runs as the body of an `async` function, as in Bruno.

**Allowed workspace dependencies:** `apifast-domain`, `apifast-app`. See the [architecture](../../docs/03-arquitectura.md) (in Spanish).

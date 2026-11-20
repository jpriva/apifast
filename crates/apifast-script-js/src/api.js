// The API scripts see, as in Bruno: `req`, `res`, `bru`, `test` and
// `expect`. It reads the input the engine gives as JSON
// (`__apifast_input`) and keeps what the script changes, which the engine
// reads back with `__apifast_output()` once the script and its tests end
// (`__apifast_settle()`).
(() => {
    const input = JSON.parse(globalThis.__apifast_input);
    const nativeSleep = globalThis.__apifast_sleep;
    const processEnv = globalThis.__apifast_process_env;
    delete globalThis.__apifast_input;
    delete globalThis.__apifast_sleep;
    delete globalThis.__apifast_process_env;

    const out = { request: null, responseBody: null, variables: [], tests: [], runner: null };
    const pending = [];

    // Variables: typed values, decoded and encoded as the files keep them.
    const decode = ({ kind, data }) => {
        switch (kind) {
            case 'number': return Number(data);
            case 'bool': return data === 'true';
            case 'object':
                try { return JSON.parse(data); } catch { return data; }
            default: return data;
        }
    };
    const encode = (value) => {
        switch (typeof value) {
            case 'string': return { kind: 'string', data: value };
            case 'number': return { kind: 'number', data: String(value) };
            case 'boolean': return { kind: 'bool', data: String(value) };
            default: return { kind: 'object', data: JSON.stringify(value) ?? 'null' };
        }
    };
    const scope = (list) => {
        const values = new Map();
        for (const variable of list) values.set(variable.name, decode(variable));
        return values;
    };
    const vars = input.variables;
    const scopes = {
        collection: scope(vars.collection),
        environment: scope(vars.environment),
        folders: scope(vars.folders),
        request: scope(vars.request),
        runtime: scope(vars.runtime),
    };
    const validName = (name) => {
        if (typeof name !== 'string' || !/^[\w.-]+$/.test(name)) {
            throw new Error(`invalid variable name ${JSON.stringify(name)}: use letters, digits, _, - and .`);
        }
    };
    const change = (scope, name, value, persist) => {
        out.variables.push({ scope, name, value: value === undefined ? null : encode(value), persist });
    };
    // From the lowest precedence to the highest, as templates resolve them.
    const lookup = (name) => {
        let found;
        for (const values of [scopes.collection, scopes.environment, scopes.folders, scopes.request, scopes.runtime]) {
            if (values.has(name)) found = { value: values.get(name) };
        }
        return found;
    };
    const interpolate = (value) => {
        if (value !== null && typeof value === 'object') return JSON.parse(interpolate(JSON.stringify(value)));
        if (typeof value !== 'string') return value;
        return value.replace(/\{\{\s*([^{}]+?)\s*\}\}/g, (whole, name) => {
            if (name.startsWith('process.env.')) return processEnv(name.slice(12)) ?? whole;
            const found = lookup(name);
            if (found === undefined) return whole;
            return typeof found.value === 'object' ? JSON.stringify(found.value) : String(found.value);
        });
    };

    // Headers: pairs, looked up by name in any case.
    const same = (a, b) => a.toLowerCase() === b.toLowerCase();
    const headerObject = (pairs) => {
        const object = {};
        for (const [name, value] of pairs) object[name] = value;
        return object;
    };
    const findHeader = (pairs, name) => pairs.find(([n]) => same(n, String(name)))?.[1];
    const isJson = (pairs) => /json/i.test(findHeader(pairs, 'content-type') ?? '');
    const parsed = (text, pairs) => {
        if (text === null || text === undefined) return undefined;
        const looksJson = /^\s*[[{]/.test(text);
        if (isJson(pairs) || (findHeader(pairs, 'content-type') === undefined && looksJson)) {
            try { return JSON.parse(text); } catch { return text; }
        }
        return text;
    };
    const text = (value) => (typeof value === 'string' ? value : JSON.stringify(value));

    // `req`: the request, before templates are rendered.
    const request = input.request;
    const touch = () => { out.request = request; };
    const req = {
        getName: () => request.name,
        getUrl: () => request.url,
        setUrl(url) { request.url = String(url); touch(); },
        getMethod: () => request.method,
        setMethod(method) { request.method = String(method).toUpperCase(); touch(); },
        getHeader: (name) => findHeader(request.headers, name),
        getHeaders: () => headerObject(request.headers),
        setHeader(name, value) {
            const index = request.headers.findIndex(([n]) => same(n, String(name)));
            const pair = [String(name), String(value)];
            if (index < 0) request.headers.push(pair); else request.headers[index] = pair;
            touch();
        },
        setHeaders(headers) {
            request.headers = Object.entries(headers).map(([n, v]) => [n, String(v)]);
            touch();
        },
        deleteHeader(name) {
            request.headers = request.headers.filter(([n]) => !same(n, String(name)));
            touch();
        },
        deleteHeaders(names) {
            for (const name of names) req.deleteHeader(name);
        },
        getBody: () => parsed(request.body, request.headers),
        setBody(body) { request.body = body === undefined || body === null ? null : text(body); touch(); },
        getTimeout: () => request.timeout_ms ?? undefined,
        setTimeout(ms) { request.timeout_ms = Math.max(0, Math.floor(Number(ms))); touch(); },
    };
    for (const [name, get, set] of [
        ['url', req.getUrl, req.setUrl],
        ['method', req.getMethod, req.setMethod],
        ['headers', req.getHeaders, req.setHeaders],
        ['body', req.getBody, req.setBody],
        ['timeout', req.getTimeout, req.setTimeout],
    ]) {
        Object.defineProperty(req, name, { get, set, enumerable: true });
    }

    // `res`: the response, and `res('path')` to read its body.
    const path = (value, query) => {
        const steps = String(query).match(/[^.[\]]+|\[\d+\]/g) ?? [];
        let current = value;
        for (const step of steps) {
            if (current === null || current === undefined) return undefined;
            current = step.startsWith('[') ? current[Number(step.slice(1, -1))] : current[step];
        }
        return current;
    };
    let res;
    if (input.response) {
        const response = input.response;
        let body = parsed(response.body, response.headers);
        res = (query) => path(body, query);
        Object.assign(res, {
            getStatus: () => response.status,
            getStatusText: () => response.status_text,
            getHeader: (name) => findHeader(response.headers, name),
            getHeaders: () => {
                const object = {};
                for (const [name, value] of response.headers) object[name.toLowerCase()] = value;
                return object;
            },
            getBody: () => body,
            setBody(value) { body = value; out.responseBody = value === undefined ? '' : text(value); },
            getResponseTime: () => response.time_ms,
            getUrl: () => response.url,
            getSize: () => {
                const header = response.headers.reduce((n, [k, v]) => n + k.length + v.length + 4, 0);
                return { header, body: response.size, total: header + response.size };
            },
        });
        for (const [name, get] of [
            ['status', res.getStatus],
            ['statusText', res.getStatusText],
            ['headers', res.getHeaders],
            ['body', res.getBody],
            ['responseTime', res.getResponseTime],
            ['url', res.getUrl],
        ]) {
            Object.defineProperty(res, name, { get, enumerable: true });
        }
    }

    // `bru`: variables, and the run.
    const runner = {
        setNextRequest(name) { out.runner = { next: String(name) }; },
        skipRequest() { out.runner = 'skip'; },
        stopExecution() { out.runner = 'stop'; },
    };
    const bru = {
        getCollectionName: () => vars.collection_name,
        getEnvName: () => vars.environment_name ?? undefined,
        getEnvVar: (name) => scopes.environment.get(name),
        hasEnvVar: (name) => scopes.environment.has(name),
        setEnvVar(name, value, options) {
            validName(name);
            scopes.environment.set(name, value);
            change('environment', name, value, Boolean(options?.persist));
        },
        deleteEnvVar(name) {
            scopes.environment.delete(name);
            change('environment', name, undefined, false);
        },
        getVar: (name) => scopes.runtime.get(name),
        hasVar: (name) => scopes.runtime.has(name),
        setVar(name, value) {
            validName(name);
            scopes.runtime.set(name, value);
            change('runtime', name, value, false);
        },
        deleteVar(name) {
            scopes.runtime.delete(name);
            change('runtime', name, undefined, false);
        },
        deleteAllVars() {
            for (const name of [...scopes.runtime.keys()]) bru.deleteVar(name);
        },
        getAllVars: () => Object.fromEntries(scopes.runtime),
        getRequestVar: (name) => scopes.request.get(name),
        getFolderVar: (name) => scopes.folders.get(name),
        getCollectionVar: (name) => scopes.collection.get(name),
        getProcessEnv: (name) => processEnv(String(name)) ?? undefined,
        interpolate,
        sleep: (ms) => nativeSleep(Math.max(0, Math.floor(Number(ms) || 0))),
        setNextRequest: runner.setNextRequest,
        runner,
    };

    // `test`: each one passes unless its function throws or rejects.
    const why = (error) => (error instanceof Error ? error.message : String(error));
    const test = (name, run) => {
        const entry = { name: String(name), error: null };
        out.tests.push(entry);
        try {
            const result = run();
            if (result && typeof result.then === 'function') {
                pending.push(Promise.resolve(result).then(() => {}, (error) => { entry.error = why(error); }));
            }
        } catch (error) {
            entry.error = why(error);
        }
    };

    Object.assign(globalThis, { req, bru, test, expect: globalThis.__apifast_expect });
    delete globalThis.__apifast_expect;
    if (res) globalThis.res = res;

    globalThis.__apifast_settle = () => Promise.all(pending);
    globalThis.__apifast_output = () => JSON.stringify(out);
})();

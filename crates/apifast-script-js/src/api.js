// The API scripts see, as in Bruno: `req`, `res`, `bru`, `test` and
// `expect`. It reads the input the engine gives as JSON
// (`__apifast_input`) and keeps what the script changes, which the engine
// reads back with `__apifast_output()` once the script and its tests end
// (`__apifast_settle()`).
(() => {
    const input = JSON.parse(globalThis.__apifast_input);
    const nativeSleep = globalThis.__apifast_sleep;
    const processEnv = globalThis.__apifast_process_env;
    const dynamicValue = globalThis.__apifast_dynamic;
    const headerList = globalThis.__apifast_header_list;
    const nativeHost = globalThis.__apifast_host;
    const cookieList = globalThis.__apifast_cookies;
    delete globalThis.__apifast_input;
    delete globalThis.__apifast_sleep;
    delete globalThis.__apifast_process_env;
    delete globalThis.__apifast_dynamic;
    delete globalThis.__apifast_header_list;
    delete globalThis.__apifast_host;
    delete globalThis.__apifast_cookies;
    delete globalThis.__apifast_schema;

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
        global: scope(vars.globals ?? []),
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
        for (const values of [scopes.global, scopes.collection, scopes.environment, scopes.folders, scopes.request, scopes.runtime]) {
            if (values.has(name)) found = { value: values.get(name) };
        }
        return found;
    };
    const interpolate = (value) => {
        if (value !== null && typeof value === 'object') return JSON.parse(interpolate(JSON.stringify(value)));
        if (typeof value !== 'string') return value;
        return value.replace(/\{\{\s*([^{}]+?)\s*\}\}/g, (whole, name) => {
            if (name.startsWith('process.env.')) return processEnv(name.slice(12)) ?? whole;
            if (name.startsWith('$')) return dynamicValue(name) ?? whole;
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

    // A URL's parts as Bruno reads them (`bruno-js/src/utils/url.js`):
    // kept as written, templates and all.
    const urlParts = (raw) => {
        if (!raw) return { host: '', pathname: '', queryString: '' };
        let rest = String(raw);
        const scheme = /^([^/?#]*):\/\//.exec(rest)?.[1]?.toLowerCase();
        rest = rest.replace(/^[^/?#]*:\/\//, '');
        if (rest.includes('#')) rest = rest.slice(0, rest.indexOf('#'));
        const cut = rest.search(/[/?]/);
        const authority = cut < 0 ? rest : rest.slice(0, cut);
        const after = cut < 0 ? '' : rest.slice(cut);
        let host = authority.slice(authority.lastIndexOf('@') + 1);
        const query = after.indexOf('?');
        let pathname = query < 0 ? after : after.slice(0, query);
        const queryString = query < 0 ? '' : after.slice(query + 1);
        // What `new URL()` would do for a URL without templates.
        if (scheme && !String(raw).includes('{{')) {
            host = host.toLowerCase().replace(scheme === 'https' ? /:443$/ : /:80$/, '');
            if (pathname === '') pathname = '/';
        }
        return { host, pathname, queryString };
    };

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
        getHost: () => urlParts(request.url).host,
        getPath() {
            const params = new Map(request.path_params ?? []);
            return urlParts(request.url).pathname
                .split('/')
                .map((segment) => {
                    const value = segment.startsWith(':') ? params.get(segment.slice(1)) : undefined;
                    return value === undefined || value.trim() === '' ? segment : value;
                })
                .join('/');
        },
        getQueryString: () => urlParts(request.url).queryString,
        getPathParams: () => (request.path_params ?? []).map(([name, value]) => ({ name, value, type: 'path' })),
        getTags: () => [...(request.tags ?? [])],
        getTimeout: () => request.timeout_ms ?? undefined,
        setTimeout(ms) { request.timeout_ms = Math.max(0, Math.floor(Number(ms))); touch(); },
    };
    req.headerList = headerList(() => request.headers, (pairs) => { request.headers = pairs; touch(); });
    for (const [name, get, set] of [
        ['url', req.getUrl, req.setUrl],
        ['method', req.getMethod, req.setMethod],
        ['headers', req.getHeaders, req.setHeaders],
        ['body', req.getBody, req.setBody],
        ['timeout', req.getTimeout, req.setTimeout],
        ['tags', req.getTags, undefined],
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
        res.headerList = headerList(() => response.headers);
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

    // Calls to the application: their answer, or the error they gave. The
    // script waits for those it did not await before it ends, as Node
    // does: `jar.setCookie(…)` without `await` still sets it.
    const inFlight = new Set();
    const hostCall = (kind, payload) => {
        const call = (async () => {
            const answer = JSON.parse(await nativeHost(kind, JSON.stringify(payload)));
            if (answer.error !== undefined) throw new Error(answer.error);
            return answer.ok;
        })();
        const settled = call.then(() => {}, () => {});
        inFlight.add(settled);
        settled.then(() => inFlight.delete(settled));
        return call;
    };
    // A response as axios gives it, which is what Bruno's scripts read.
    const axiosResponse = (raw) => {
        const headers = {};
        for (const [name, value] of raw.headers) headers[name.toLowerCase()] = value;
        const data = parsed(raw.body, raw.headers);
        return {
            status: raw.status, statusText: raw.status_text, headers,
            data: data === undefined ? '' : data, duration: raw.time_ms, url: raw.url,
        };
    };
    // `bru.sendRequest(urlOrOptions, callback?)`: axios' options (`url`,
    // `method`, `headers`, `params`, `data`, `timeout`). A status out of
    // 2xx rejects, with `status` and `response`, as in axios.
    const sendRequest = (options, callback) => {
        const config = typeof options === 'string' ? { url: options } : { ...(options ?? {}) };
        let url = String(config.url ?? '');
        const params = config.params && typeof config.params === 'object'
            ? Object.entries(config.params).filter(([, v]) => v !== undefined && v !== null)
            : [];
        if (params.length > 0) {
            const query = params.map(([k, v]) => `${encodeURIComponent(k)}=${encodeURIComponent(String(v))}`).join('&');
            url += (url.includes('?') ? '&' : '?') + query;
        }
        const headers = Object.entries(config.headers ?? {}).map(([n, v]) => [n, String(v)]);
        let body = null;
        if (config.data !== undefined && config.data !== null) {
            if (typeof config.data === 'string') {
                body = config.data;
            } else {
                body = JSON.stringify(config.data);
                if (!headers.some(([n]) => same(n, 'content-type'))) headers.push(['Content-Type', 'application/json']);
            }
        }
        const timeout = Number(config.timeout);
        const promise = hostCall('send', {
            method: String(config.method ?? 'GET').toUpperCase(),
            url,
            headers,
            body,
            timeout_ms: Number.isFinite(timeout) && timeout > 0 ? Math.floor(timeout) : null,
        }).then((raw) => {
            const response = axiosResponse(raw);
            if (response.status < 200 || response.status >= 300) {
                const error = new Error(`Request failed with status code ${response.status}`);
                Object.assign(error, { status: response.status, response });
                throw error;
            }
            return response;
        });
        if (typeof callback === 'function') {
            return promise.then((response) => callback(null, response), (error) => callback(error, null));
        }
        return promise;
    };
    // `bru.runRequest(path)`: a request of the collection, sent with its
    // scripts; what they set is seen here, as in Bruno.
    const runRequest = async (path) => {
        const ran = await hostCall('run', String(path));
        for (const changed of ran.variables) {
            const values = scopes[changed.scope];
            if (values) {
                if (changed.value === null) values.delete(changed.name); else values.set(changed.name, decode(changed.value));
            }
            out.variables.push(changed);
        }
        return axiosResponse(ran.response);
    };

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
        hasCollectionVar: (name) => scopes.collection.has(name),
        setCollectionVar(name, value) {
            validName(name);
            scopes.collection.set(name, value);
            change('collection', name, value, false);
        },
        deleteCollectionVar(name) {
            scopes.collection.delete(name);
            change('collection', name, undefined, false);
        },
        getAllCollectionVars: () => Object.fromEntries(scopes.collection),
        deleteAllCollectionVars() {
            for (const name of [...scopes.collection.keys()]) bru.deleteCollectionVar(name);
        },
        getAllEnvVars: () => Object.fromEntries(scopes.environment),
        deleteAllEnvVars() {
            for (const name of [...scopes.environment.keys()]) bru.deleteEnvVar(name);
        },
        getGlobalEnvVar: (name) => scopes.global.get(name),
        hasGlobalEnvVar: (name) => scopes.global.has(name),
        setGlobalEnvVar(name, value) {
            validName(name);
            scopes.global.set(name, value);
            change('global', name, value, false);
        },
        deleteGlobalEnvVar(name) {
            scopes.global.delete(name);
            change('global', name, undefined, false);
        },
        getAllGlobalEnvVars: () => Object.fromEntries(scopes.global),
        deleteAllGlobalEnvVars() {
            for (const name of [...scopes.global.keys()]) bru.deleteGlobalEnvVar(name);
        },
        // Scripts reach neither files nor the network: always safe mode.
        isSafeMode: () => true,
        getProcessEnv: (name) => processEnv(String(name)) ?? undefined,
        interpolate,
        sleep: (ms) => nativeSleep(Math.max(0, Math.floor(Number(ms) || 0))),
        sendRequest,
        runRequest,
        cookies: cookieList(
            input.cookies,
            (change) => hostCall('cookies', change),
            () => request.url,
            (url) => interpolate(String(url)),
        ),
        setNextRequest: runner.setNextRequest,
        runner,
    };

    // Timers. A pending timeout keeps the script running until it fires
    // (within the time limit), as in Node; an interval does not.
    const timers = new Map();
    let lastTimer = 0;
    const delay = (ms) => nativeSleep(Math.max(0, Math.floor(Number(ms) || 0)));
    const call = (fn, args) => { if (typeof fn === 'function') fn(...args); };
    const setTimeout = (fn, ms, ...args) => {
        const id = ++lastTimer;
        const fired = delay(ms).then(() => {
            if (timers.delete(id)) call(fn, args);
        });
        timers.set(id, fired);
        return id;
    };
    const setInterval = (fn, ms, ...args) => {
        const id = ++lastTimer;
        timers.set(id, null);
        (async () => {
            while (timers.has(id)) {
                await delay(ms);
                if (timers.has(id)) call(fn, args);
            }
        })();
        return id;
    };
    const clearTimer = (id) => { timers.delete(id); };
    const timeouts = async () => {
        let waiting;
        while ((waiting = [...timers.values()].filter(Boolean)).length > 0) await Promise.all(waiting);
    };

    // `test`: each one passes unless its function throws or rejects.
    const why = (error) => (error instanceof Error ? error.message : String(error));
    // It gives a promise that resolves once the test ends, and never
    // rejects: `await test(…)` runs the next one after it, as in Bruno.
    const test = (name, run) => {
        const entry = { name: String(name), error: null };
        out.tests.push(entry);
        try {
            const result = run();
            if (result && typeof result.then === 'function') {
                const ended = Promise.resolve(result).then(() => {}, (error) => { entry.error = why(error); });
                pending.push(ended);
                return ended;
            }
        } catch (error) {
            entry.error = why(error);
        }
        return Promise.resolve();
    };

    Object.assign(globalThis, {
        req, bru, test, expect: globalThis.__apifast_expect,
        setTimeout, setInterval, clearTimeout: clearTimer, clearInterval: clearTimer,
    });
    delete globalThis.__apifast_expect;
    if (res) globalThis.res = res;

    globalThis.__apifast_settle = async () => {
        // What is still waiting can start more: until nothing is.
        for (;;) {
            const tests = pending.length;
            await timeouts();
            await Promise.all([...inFlight]);
            await Promise.all(pending);
            const idle = inFlight.size === 0 && ![...timers.values()].some(Boolean);
            if (idle && pending.length === tests) break;
        }
    };
    globalThis.__apifast_output = () => JSON.stringify(out);
})();

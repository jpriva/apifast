// `bru.cookies`: the cookies of the current request's URL, as Bruno's
// `CookieList`, and `bru.cookies.jar()` for those of any URL, as its jar
// (tough-cookie's, through `@usebruno/requests`). Reads are synchronous,
// over a copy of the collection's jar the script is given; changes go to
// the application, which answers with the jar as it is then. It leaves
// itself in `__apifast_cookies(initial, change, currentUrl, interpolate)`:
// `change(payload)` is a promise of the jar after the change.
(() => {
    const isIp = (host) => /^\d{1,3}(?:\.\d{1,3}){3}$/.test(host) || host.includes(':');
    const isLocal = (host) => host === 'localhost' || host.endsWith('.localhost')
        || /^127\.\d{1,3}\.\d{1,3}\.\d{1,3}$/.test(host) || host === '::1';
    // Where `url` goes, as Rust's `CookieTarget::from_url` reads it.
    const target = (url) => {
        const match = /^([^:/?#]+):\/\/([^/?#]*)([^?#]*)/.exec(String(url ?? '').trim());
        if (!match) return null;
        const authority = match[2].slice(match[2].lastIndexOf('@') + 1);
        const host = (authority.startsWith('[')
            ? authority.slice(1, authority.indexOf(']'))
            : authority.split(':')[0]).toLowerCase();
        if (!host) return null;
        return { host, path: match[3], secure: match[1].toLowerCase() === 'https' || isLocal(host) };
    };
    const domainMatches = (host, domain) => host === domain || (host.endsWith(`.${domain}`) && !isIp(host));
    const pathMatches = (path, cookiePath) => {
        const p = path || '/';
        return p === cookiePath
            || (p.startsWith(cookiePath) && (cookiePath.endsWith('/') || p[cookiePath.length] === '/'));
    };
    const goes = (raw, at, now) => (raw.expires_ms === null || raw.expires_ms > now)
        && (raw.host_only ? at.host === raw.domain : domainMatches(at.host, raw.domain))
        && pathMatches(at.path, raw.path)
        && (!raw.secure || at.secure);
    // A cookie as tough-cookie gives it to scripts.
    const shown = (raw) => ({
        key: raw.name,
        value: raw.value,
        domain: raw.domain,
        path: raw.path,
        secure: raw.secure,
        httpOnly: raw.http_only,
        hostOnly: raw.host_only,
        sameSite: raw.same_site ? raw.same_site.toLowerCase() : undefined,
        expires: raw.expires_ms === null ? 'Infinity' : new Date(raw.expires_ms).toISOString(),
        creation: new Date(raw.created_ms).toISOString(),
    });
    // What a script gives to set a cookie, as the application takes it.
    const spec = (object) => {
        const name = object.key ?? object.name;
        if (!name) throw new Error('cookieObject.key (name) is required');
        let expires = null;
        if (object.maxAge !== undefined && object.maxAge !== null && object.maxAge !== 'Infinity') {
            expires = Date.now() + Number(object.maxAge) * 1000;
        } else if (object.expires instanceof Date) {
            expires = object.expires.getTime();
        } else if (typeof object.expires === 'number') {
            expires = object.expires;
        } else if (typeof object.expires === 'string' && object.expires !== 'Infinity') {
            const parsed = Date.parse(object.expires);
            expires = Number.isNaN(parsed) ? null : parsed;
        }
        return {
            name: String(name),
            value: object.value === undefined || object.value === null ? '' : String(object.value),
            domain: object.domain ? String(object.domain) : null,
            path: object.path ? String(object.path) : null,
            secure: Boolean(object.secure),
            http_only: Boolean(object.httpOnly),
            same_site: object.sameSite ? String(object.sameSite) : null,
            expires_ms: expires === null || !Number.isFinite(expires) ? null : Math.floor(expires),
        };
    };
    // A promise, or the node-style callback it ends in.
    const settle = (work, callback) => {
        const promise = Promise.resolve().then(work);
        if (typeof callback !== 'function') return promise;
        promise.then((value) => callback(null, value), (error) => callback(error));
        return undefined;
    };
    const required = (value, message) => {
        if (!value) throw new Error(message);
    };

    const cookies = (initial, change, currentUrl, interpolate) => {
        let jar = initial;
        const apply = async (payload) => { jar = await change(payload); };
        const at = (url) => {
            const where = target(url);
            if (!where) throw new Error(`\`${url}\` is not a URL with a scheme and a host`);
            return where;
        };
        const going = (url) => {
            const where = at(url);
            const now = Date.now();
            return jar.filter((raw) => goes(raw, where, now))
                .sort((a, b) => b.path.length - a.path.length || a.created_ms - b.created_ms)
                .map(shown);
        };

        const jarApi = {
            getCookie: (url, name, callback) => settle(() => {
                required(url && name, 'URL and cookie name are required');
                return going(interpolate(url)).find((c) => c.key === name) ?? null;
            }, callback),
            hasCookie: (url, name, callback) => settle(() => {
                required(url && name, 'URL and cookie name are required');
                return going(interpolate(url)).some((c) => c.key === name);
            }, callback),
            getCookies: (url, callback) => settle(() => {
                required(url, 'URL is required');
                return going(interpolate(url));
            }, callback),
            setCookie(url, nameOrObject, valueOrCallback, maybeCallback) {
                const callback = typeof maybeCallback === 'function' ? maybeCallback
                    : (typeof valueOrCallback === 'function' ? valueOrCallback : undefined);
                return settle(async () => {
                    required(url, 'URL is required');
                    let object;
                    if (typeof nameOrObject === 'string') {
                        required(nameOrObject, 'Cookie name is required');
                        object = { key: nameOrObject, value: typeof valueOrCallback === 'string' ? valueOrCallback : '' };
                    } else if (nameOrObject && typeof nameOrObject === 'object') {
                        object = nameOrObject;
                    } else {
                        throw new Error('Invalid arguments passed to setCookie');
                    }
                    const where = interpolate(url);
                    at(where);
                    await apply({ op: 'set', url: where, cookie: spec(object) });
                }, callback);
            },
            setCookies: (url, list, callback) => settle(async () => {
                required(url, 'URL is required');
                if (!Array.isArray(list)) throw new Error('setCookies expects an array of cookie objects');
                const where = interpolate(url);
                at(where);
                for (const object of list) await apply({ op: 'set', url: where, cookie: spec(object) });
            }, callback),
            clear: (callback) => settle(() => apply({ op: 'clear' }), callback),
            deleteCookies: (url, callback) => settle(() => {
                required(url, 'URL is required');
                return apply({ op: 'delete', url: interpolate(url), name: null });
            }, callback),
            deleteCookie: (url, name, callback) => settle(() => {
                required(url && name, 'URL and cookie name are required');
                return apply({ op: 'delete', url: interpolate(url), name: String(name) });
            }, callback),
        };

        // The current request's cookies, as a property list.
        const current = () => {
            const url = interpolate(currentUrl());
            return target(url) ? going(url) : [];
        };
        const bind = (fn, context) => (context !== undefined ? fn.bind(context) : fn);
        const writeCurrent = (payload, callback) => {
            const url = interpolate(currentUrl());
            if (!target(url)) return settle(() => {}, callback);
            return settle(() => apply({ ...payload, url }), callback);
        };
        const list = {
            get: (name) => list.one(name)?.value,
            one: (name) => current().filter((c) => c.key === name).at(-1),
            all: () => current(),
            idx: (index) => current()[index],
            count: () => current().length,
            indexOf(nameOrItem) {
                if (typeof nameOrItem === 'string') return current().findIndex((c) => c.key === nameOrItem);
                if (!nameOrItem || typeof nameOrItem !== 'object') return -1;
                return current().findIndex((c) => c.key === nameOrItem.key && c.value === nameOrItem.value);
            },
            has(nameOrItem, value) {
                const name = nameOrItem && typeof nameOrItem === 'object' ? nameOrItem.key : nameOrItem;
                return current().some((c) => c.key === name && (value === undefined || c.value === value));
            },
            find: (predicate, context) => current().find(bind(predicate, context)),
            filter: (predicate, context) => current().filter(bind(predicate, context)),
            each(fn, context) { current().forEach(bind(fn, context)); },
            map: (fn, context) => current().map(bind(fn, context)),
            reduce(fn, ...rest) {
                const bound = bind(fn, rest[1]);
                return rest.length ? current().reduce(bound, rest[0]) : current().reduce(bound);
            },
            toObject() {
                const object = {};
                for (const c of current()) object[c.key] = c.value;
                return object;
            },
            toString: () => current().map((c) => `${c.key}=${c.value}`).join('; '),
            toJSON: () => current(),
            upsert(object, callback) {
                if (!object || typeof object !== 'object') {
                    const error = new Error('cookieObj must be a non-null object');
                    if (callback) return callback(error);
                    return Promise.reject(error);
                }
                return settle(() => writeCurrent({ op: 'set', cookie: spec(object) }), callback);
            },
            add: (object, callback) => list.upsert(object, callback),
            append: (object, callback) => list.upsert(object, callback),
            remove(name, callback) {
                if (!name) return settle(() => {}, callback);
                return writeCurrent({ op: 'delete', name: String(name) }, callback);
            },
            delete: (name, callback) => list.remove(name, callback),
            clear: (callback) => writeCurrent({ op: 'delete', name: null }, callback),
            jar: () => jarApi,
        };
        return list;
    };

    globalThis.__apifast_cookies = cookies;
})();

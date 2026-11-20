// `expect`, the part of chai's BDD style that scripts use, with chai's
// messages: `expected 404 to equal 200`. It leaves itself in
// `__apifast_expect`, for the API to publish.
(() => {
    const schemas = globalThis.__apifast_schema;

    class AssertionError extends Error {
        constructor(message) {
            super(message);
            this.name = 'AssertionError';
        }
    }

    const typeOf = (value) => {
        if (value === null) return 'null';
        if (Array.isArray(value)) return 'array';
        if (value instanceof Date) return 'date';
        if (value instanceof RegExp) return 'regexp';
        if (value instanceof Error) return 'error';
        return typeof value;
    };
    const inspect = (value) => {
        switch (typeOf(value)) {
            case 'string': return `'${value}'`;
            case 'undefined': return 'undefined';
            case 'function': return `[Function${value.name ? ` ${value.name}` : ''}]`;
            case 'regexp': return String(value);
            case 'error': return `[${value.name}: ${value.message}]`;
            case 'number': return Object.is(value, -0) ? '-0' : String(value);
            default:
                try {
                    const json = JSON.stringify(value);
                    return json === undefined ? String(value) : json;
                } catch {
                    return String(value);
                }
        }
    };
    const deepEqual = (a, b) => {
        if (Object.is(a, b)) return true;
        if (typeof a !== 'object' || typeof b !== 'object' || a === null || b === null) return false;
        if (Object.getPrototypeOf(a) !== Object.getPrototypeOf(b)) return false;
        if (a instanceof Date) return a.getTime() === b.getTime();
        if (a instanceof RegExp) return String(a) === String(b);
        const keys = Object.keys(a);
        if (keys.length !== Object.keys(b).length) return false;
        return keys.every((key) => Object.prototype.hasOwnProperty.call(b, key) && deepEqual(a[key], b[key]));
    };
    const lengthOf = (value) => {
        if (value instanceof Map || value instanceof Set) return value.size;
        return value?.length;
    };

    function Assertion(object, message) {
        this._object = object;
        this._negate = false;
        this._deep = false;
        this._nested = false;
        this._own = false;
        this._any = false;
        this._contains = false;
        this._message = message;
    }
    const proto = Assertion.prototype;

    // Fails unless `ok` (or, after `.not`, unless it is false), with
    // `message` (or `negated`), where #{this}, #{exp} and #{act} are
    // replaced.
    proto.assert = function (ok, message, negated, expected, actual = this._object) {
        if (this._negate ? !ok : ok) return this;
        const fill = (text) => text
            .replace(/#\{this\}/g, () => inspect(this._object))
            .replace(/#\{exp\}/g, () => inspect(expected))
            .replace(/#\{act\}/g, () => inspect(actual));
        const prefix = this._message ? `${this._message}: ` : '';
        throw new AssertionError(prefix + fill(this._negate ? negated : message));
    };

    const chain = (name, apply = () => {}) => {
        Object.defineProperty(proto, name, {
            get() { apply(this); return this; },
            configurable: true,
        });
    };
    for (const name of ['to', 'be', 'been', 'is', 'that', 'which', 'and', 'has', 'have', 'with',
        'at', 'of', 'same', 'but', 'does', 'still', 'also']) {
        chain(name);
    }
    chain('not', (a) => { a._negate = !a._negate; });
    chain('deep', (a) => { a._deep = true; });
    chain('nested', (a) => { a._nested = true; });
    chain('own', (a) => { a._own = true; });
    chain('any', (a) => { a._any = true; });
    chain('all', (a) => { a._any = false; });

    // Properties that assert.
    const check = (name, test, message, negated) => {
        Object.defineProperty(proto, name, {
            get() { return this.assert(test(this._object), message, negated); },
            configurable: true,
        });
    };
    check('ok', (v) => Boolean(v), 'expected #{this} to be truthy', 'expected #{this} to be falsy');
    check('true', (v) => v === true, 'expected #{this} to be true', 'expected #{this} to not be true');
    check('false', (v) => v === false, 'expected #{this} to be false', 'expected #{this} to not be false');
    check('null', (v) => v === null, 'expected #{this} to be null', 'expected #{this} not to be null');
    check('undefined', (v) => v === undefined, 'expected #{this} to be undefined', 'expected #{this} not to be undefined');
    check('NaN', (v) => Number.isNaN(v), 'expected #{this} to be NaN', 'expected #{this} not to be NaN');
    check('exist', (v) => v !== null && v !== undefined, 'expected #{this} to exist', 'expected #{this} to not exist');
    check('finite', (v) => Number.isFinite(v), 'expected #{this} to be a finite number', 'expected #{this} to not be a finite number');
    check('empty', (v) => {
        if (typeof v === 'string' || Array.isArray(v)) return v.length === 0;
        if (v instanceof Map || v instanceof Set) return v.size === 0;
        if (v !== null && typeof v === 'object') return Object.keys(v).length === 0;
        throw new AssertionError(`.empty was passed non-string primitive ${inspect(v)}`);
    }, 'expected #{this} to be empty', 'expected #{this} not to be empty');

    const method = (names, body) => {
        for (const name of names) proto[name] = body;
    };

    method(['equal', 'equals', 'eq'], function (expected) {
        if (this._deep) return this.eql(expected);
        return this.assert(this._object === expected,
            'expected #{this} to equal #{exp}', 'expected #{this} to not equal #{exp}', expected);
    });
    method(['eql', 'eqls'], function (expected) {
        return this.assert(deepEqual(this._object, expected),
            'expected #{this} to deeply equal #{exp}', 'expected #{this} to not deeply equal #{exp}', expected);
    });
    method(['a', 'an'], function (type) {
        const wanted = String(type).toLowerCase();
        const article = /^[aeiou]/.test(wanted) ? 'an' : 'a';
        return this.assert(typeOf(this._object) === wanted,
            `expected #{this} to be ${article} ${wanted}`, `expected #{this} not to be ${article} ${wanted}`);
    });
    method(['include', 'includes', 'contain', 'contains'], function (expected) {
        const object = this._object;
        const equal = this._deep ? deepEqual : (a, b) => a === b;
        let ok;
        if (typeof object === 'string') {
            ok = object.includes(expected);
        } else if (Array.isArray(object)) {
            ok = object.some((item) => equal(item, expected));
        } else if (object instanceof Set) {
            ok = [...object].some((item) => equal(item, expected));
        } else if (object instanceof Map) {
            ok = [...object.values()].some((item) => equal(item, expected));
        } else if (object !== null && typeof object === 'object' && expected !== null && typeof expected === 'object') {
            ok = Object.keys(expected).every((key) => key in object && equal(object[key], expected[key]));
        } else {
            throw new AssertionError(`object tested must be an array, a map, an object, a set or a string, but ${typeOf(object)} given`);
        }
        return this.assert(ok, 'expected #{this} to include #{exp}', 'expected #{this} to not include #{exp}', expected);
    });
    const compare = (names, test, word) => method(names, function (bound) {
        return this.assert(test(this._object, bound),
            `expected #{this} to be ${word} #{exp}`, `expected #{this} to be not ${word} #{exp}`, bound);
    });
    compare(['above', 'gt', 'greaterThan'], (a, b) => a > b, 'above');
    compare(['least', 'gte', 'greaterThanOrEqual'], (a, b) => a >= b, 'at least');
    compare(['below', 'lt', 'lessThan'], (a, b) => a < b, 'below');
    compare(['most', 'lte', 'lessThanOrEqual'], (a, b) => a <= b, 'at most');
    method(['within'], function (low, high) {
        const value = this._object;
        return this.assert(value >= low && value <= high,
            `expected #{this} to be within ${low}..${high}`, `expected #{this} to not be within ${low}..${high}`);
    });
    method(['closeTo', 'approximately'], function (expected, delta) {
        return this.assert(Math.abs(this._object - expected) <= delta,
            `expected #{this} to be close to ${expected} +/- ${delta}`,
            `expected #{this} not to be close to ${expected} +/- ${delta}`);
    });
    method(['lengthOf', 'length'], function (expected) {
        const length = lengthOf(this._object);
        return this.assert(length === expected,
            `expected #{this} to have a length of #{exp} but got ${length}`,
            'expected #{this} to not have a length of #{exp}', expected, length);
    });
    method(['property'], function (name, ...value) {
        const object = this._object;
        let found = false;
        let actual;
        if (this._nested) {
            const steps = String(name).match(/[^.[\]]+|\[\d+\]/g) ?? [];
            let current = object;
            found = true;
            for (const step of steps) {
                const key = step.startsWith('[') ? Number(step.slice(1, -1)) : step;
                if (current === null || current === undefined || !(key in Object(current))) {
                    found = false;
                    break;
                }
                current = current[key];
            }
            actual = current;
        } else if (object !== null && object !== undefined) {
            found = this._own ? Object.prototype.hasOwnProperty.call(object, name) : name in Object(object);
            actual = found ? object[name] : undefined;
        }
        const what = `${this._nested ? 'nested ' : ''}${this._own ? 'own ' : ''}property`;
        if (value.length === 0) {
            this.assert(found, `expected #{this} to have ${what} '${name}'`, `expected #{this} to not have ${what} '${name}'`);
        } else {
            const equal = this._deep ? deepEqual(actual, value[0]) : actual === value[0];
            this.assert(found && equal,
                `expected #{this} to have ${what} '${name}' of #{exp}, but got #{act}`,
                `expected #{this} to not have ${what} '${name}' of #{exp}`, value[0], actual);
        }
        const next = new Assertion(actual, this._message);
        return next;
    });
    method(['ownProperty', 'haveOwnProperty'], function (name, ...value) {
        this._own = true;
        return this.property(name, ...value);
    });
    method(['match', 'matches'], function (pattern) {
        return this.assert(pattern.test(String(this._object)),
            'expected #{this} to match #{exp}', 'expected #{this} not to match #{exp}', pattern);
    });
    method(['string'], function (part) {
        return this.assert(String(this._object).includes(part),
            'expected #{this} to contain #{exp}', 'expected #{this} to not contain #{exp}', part);
    });
    method(['keys', 'key'], function (...wanted) {
        let keys = wanted.length === 1 && Array.isArray(wanted[0]) ? wanted[0] : wanted;
        if (keys.length === 1 && keys[0] !== null && typeof keys[0] === 'object' && !Array.isArray(keys[0])) {
            keys = Object.keys(keys[0]);
        }
        const object = this._object;
        const actual = object instanceof Map ? [...object.keys()] : Object.keys(object ?? {});
        const present = keys.filter((key) => actual.includes(key));
        const ok = this._any
            ? present.length > 0
            : present.length === keys.length && (this._contains || actual.length === keys.length);
        const list = keys.map(inspect).join(', ');
        return this.assert(ok,
            `expected #{this} to have ${this._any ? 'any of' : 'all of'} keys ${list}`,
            `expected #{this} to not have ${this._any ? 'any of' : 'all of'} keys ${list}`);
    });
    method(['members'], function (expected) {
        const actual = this._object;
        const equal = this._deep ? deepEqual : (a, b) => a === b;
        const all = expected.every((item) => actual.some((other) => equal(item, other)));
        const ok = this._contains ? all : all && actual.length === expected.length
            && actual.every((item) => expected.some((other) => equal(item, other)));
        return this.assert(ok,
            `expected #{this} to have the same members as #{exp}`,
            `expected #{this} to not have the same members as #{exp}`, expected);
    });
    method(['oneOf'], function (list) {
        const equal = this._deep ? deepEqual : (a, b) => a === b;
        return this.assert(list.some((item) => equal(item, this._object)),
            'expected #{this} to be one of #{exp}', 'expected #{this} to not be one of #{exp}', list);
    });
    method(['instanceOf', 'instanceof'], function (type) {
        const name = type?.name ?? String(type);
        return this.assert(this._object instanceof type,
            `expected #{this} to be an instance of ${name}`, `expected #{this} to not be an instance of ${name}`);
    });
    method(['satisfy', 'satisfies'], function (test) {
        return this.assert(Boolean(test(this._object)),
            `expected #{this} to satisfy ${inspect(test)}`, `expected #{this} to not satisfy ${inspect(test)}`);
    });
    method(['throw', 'throws', 'Throw'], function (expected, message) {
        let thrown;
        let threw = false;
        try {
            this._object();
        } catch (error) {
            threw = true;
            thrown = error;
        }
        let ok = threw;
        let wanted = '';
        if (threw && typeof expected === 'function') {
            ok = thrown instanceof expected;
            wanted = ` ${expected.name}`;
        }
        const text = typeof expected === 'string' || expected instanceof RegExp ? expected : message;
        if (ok && text !== undefined) {
            const got = thrown instanceof Error ? thrown.message : String(thrown);
            ok = text instanceof RegExp ? text.test(got) : got.includes(text);
            wanted += ` including ${inspect(text)}`;
        }
        return this.assert(ok, `expected #{this} to throw${wanted}`, `expected #{this} to not throw${wanted}`);
    });

    // Bruno's own: `jsonSchema` (draft-07, as ajv checks it) and
    // `jsonBody` (Postman's).
    const SCHEMA_VERSIONS = ['http://json-schema.org/draft-07/schema#', 'http://json-schema.org/draft-07/schema'];
    method(['jsonSchema'], function (schema, options) {
        if (schema && schema.$schema && !SCHEMA_VERSIONS.includes(schema.$schema)) {
            this.assert(
                false,
                `Unsupported JSON Schema version: "${schema.$schema}". Only Draft-07 (http://json-schema.org/draft-07/schema#) is supported.`,
                `Unsupported JSON Schema version: "${schema.$schema}".`,
            );
        }
        let validate;
        try {
            validate = schemas.compile(schema, { allErrors: true, ...options });
        } catch (error) {
            throw new AssertionError(`JSON schema compile error: ${error.message}`);
        }
        const errors = validate(this._object);
        return this.assert(
            errors.length === 0,
            `expected #{this} to match JSON schema, validation errors: ${JSON.stringify(errors)}`,
            'expected #{this} to not match JSON schema',
        );
    });
    // `a.b`, `items[0]`, `data["a.b"]`, `m[0][1]`: the keys of a path.
    const pathKeys = (path) => {
        const keys = [];
        let i = 0;
        while (i < path.length) {
            if (path[i] === '.') {
                i += 1;
            } else if (path[i] === '[') {
                i += 1;
                let key = '';
                const quote = path[i] === "'" || path[i] === '"' ? path[i] : null;
                if (quote) {
                    i += 1;
                    while (i < path.length && path[i] !== quote) {
                        if (path[i] === '\\' && path[i + 1] === quote) { key += quote; i += 2; } else { key += path[i]; i += 1; }
                    }
                    i += 1;
                } else {
                    while (i < path.length && path[i] !== ']') { key += path[i]; i += 1; }
                }
                i += 1;
                keys.push(key);
            } else {
                let key = '';
                while (i < path.length && path[i] !== '.' && path[i] !== '[') { key += path[i]; i += 1; }
                keys.push(key);
            }
        }
        return keys;
    };
    const atPath = (value, path) => {
        let current = value;
        for (const key of pathKeys(path)) {
            if (current === null || current === undefined || !Object.prototype.hasOwnProperty.call(Object(current), key)) {
                return { found: false };
            }
            current = current[key];
        }
        return { found: true, value: current };
    };
    method(['jsonBody'], function (...args) {
        const body = this._object;
        if (args.length === 0) {
            return this.assert(typeof body === 'object' && body !== null,
                'expected value to be a JSON body (object or array)', 'expected value not to be a JSON body');
        }
        if (args.length === 1 && typeof args[0] === 'object' && args[0] !== null) {
            return this.assert(deepEqual(body, args[0]),
                'expected body to deeply equal given object', 'expected body to not deeply equal given object');
        }
        const found = atPath(body, String(args[0]));
        if (args.length === 1) {
            return this.assert(found.found,
                `expected body to have nested property '${args[0]}'`, `expected body to not have nested property '${args[0]}'`);
        }
        return this.assert(found.found && deepEqual(found.value, args[1]),
            `expected body to have nested property '${args[0]}' equal to given value`,
            `expected body to not have nested property '${args[0]}' equal to given value`);
    });

    // `.contain.keys(…)` and `.include.members(…)`: as properties, they
    // only mark that a part is enough.
    for (const name of ['include', 'contain']) {
        const call = proto[name];
        Object.defineProperty(proto, name, {
            get() {
                const assertion = this;
                const include = (...args) => call.apply(assertion, args);
                return new Proxy(include, {
                    get(_, key) {
                        assertion._contains = true;
                        const value = assertion[key];
                        return typeof value === 'function' ? value.bind(assertion) : value;
                    },
                });
            },
            configurable: true,
        });
    }

    const expect = (value, message) => new Assertion(value, message);
    expect.fail = (message = 'expect.fail()') => { throw new AssertionError(message); };
    expect.AssertionError = AssertionError;
    globalThis.__apifast_expect = expect;
})();

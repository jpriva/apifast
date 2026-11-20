// JSON Schema draft-07, as `expect(…).to.have.jsonSchema(schema, options)`
// checks it in Bruno, which uses ajv with ajv-formats. The options apifast
// honours are ajv's `strict` (on by default: an unknown keyword or format
// is a compile error), `coerceTypes` and `allErrors` (errors are always all
// collected). It leaves itself in `__apifast_schema`, for `expect` to use.
(() => {
    const TYPES = ['null', 'boolean', 'object', 'array', 'number', 'integer', 'string'];
    const ANNOTATIONS = ['$schema', '$id', '$comment', 'title', 'description', 'default', 'examples',
        'readOnly', 'writeOnly', 'contentMediaType', 'contentEncoding', 'definitions'];
    // How each keyword holds subschemas: one, a list, or a map of them.
    const ONE = ['additionalProperties', 'additionalItems', 'contains', 'propertyNames', 'not', 'if', 'then', 'else'];
    const LIST = ['allOf', 'anyOf', 'oneOf'];
    const MAP = ['properties', 'patternProperties', 'definitions'];
    const KEYWORDS = new Set([...ANNOTATIONS, ...ONE, ...LIST, ...MAP, 'type', 'enum', 'const', 'required',
        'items', 'dependencies', 'minItems', 'maxItems', 'uniqueItems', 'minimum', 'maximum',
        'exclusiveMinimum', 'exclusiveMaximum', 'multipleOf', 'minLength', 'maxLength', 'pattern',
        'format', 'minProperties', 'maxProperties', '$ref']);

    const ipv6 = (text) => {
        const parts = text.split('::');
        if (parts.length > 2) return false;
        const groups = (part) => (part === '' ? [] : part.split(':'));
        const head = groups(parts[0]);
        const tail = parts.length === 2 ? groups(parts[1]) : [];
        const all = [...head, ...tail];
        let count = all.length;
        const last = all[all.length - 1];
        if (last !== undefined && last.includes('.')) {
            if (!FORMATS.ipv4(last)) return false;
            all.pop();
            count += 1;
        }
        if (!all.every((g) => /^[0-9a-f]{1,4}$/i.test(g))) return false;
        return parts.length === 2 ? count < 8 : count === 8;
    };
    const date = (text) => {
        const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(text);
        if (!match) return false;
        const [year, month, day] = [Number(match[1]), Number(match[2]), Number(match[3])];
        const leap = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
        const days = [31, leap ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][month - 1];
        return month >= 1 && month <= 12 && day >= 1 && day <= days;
    };
    const time = (text) => /^(?:[01]\d|2[0-3]):[0-5]\d:(?:[0-5]\d|60)(?:\.\d+)?(?:z|[+-](?:[01]\d|2[0-3]):?[0-5]\d)$/i.test(text);
    // RFC 3986: an optional scheme and authority, then the path, query and
    // fragment, with only the characters a URI allows.
    const URI_REFERENCE = /^(?:[a-z][a-z0-9+\-.]*:)?(?:\/\/(?:(?:[a-z0-9\-._~!$&'()*+,;=:]|%[0-9a-f]{2})*@)?(?:\[[0-9a-f:.]+\]|(?:[a-z0-9\-._~!$&'()*+,;=]|%[0-9a-f]{2})*)(?::\d*)?)?(?:[a-z0-9\-._~!$&'()*+,;=:@/]|%[0-9a-f]{2})*(?:\?(?:[a-z0-9\-._~!$&'()*+,;=:@/?]|%[0-9a-f]{2})*)?(?:#(?:[a-z0-9\-._~!$&'()*+,;=:@/?]|%[0-9a-f]{2})*)?$/i;
    // Those of strings; the numeric ones check numbers.
    const FORMATS = {
        date,
        time,
        'date-time': (text) => {
            const parts = text.split(/t|\s/i);
            return parts.length === 2 && date(parts[0]) && time(parts[1]);
        },
        duration: (text) => /^P(?!$)((\d+Y)?(\d+M)?(\d+D)?(T(?=\d)(\d+H)?(\d+M)?(\d+S)?)?|(\d+W)?)$/.test(text),
        uri: (text) => /^[a-z][a-z0-9+\-.]*:/i.test(text) && URI_REFERENCE.test(text),
        url: (text) => /^(?:https?|ftp):\/\/[^\s/$.?#][^\s]*$/i.test(text),
        'uri-reference': (text) => URI_REFERENCE.test(text),
        'uri-template': (text) => /^(?:(?:[^\x00-\x20"'<>%\\^`{|}]|%[0-9a-f]{2})|\{[+#./;?&=,!@|]?(?:[a-z0-9_]|%[0-9a-f]{2})+(?::[1-9][0-9]{0,3}|\*)?(?:,(?:[a-z0-9_]|%[0-9a-f]{2})+(?::[1-9][0-9]{0,3}|\*)?)*\})*$/i.test(text),
        email: (text) => /^[a-z0-9.!#$%&'*+/=?^_`{|}~-]+@[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?)*$/i.test(text),
        hostname: (text) => /^(?=.{1,253}\.?$)[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?(?:\.[a-z0-9](?:[-0-9a-z]{0,61}[0-9a-z])?)*\.?$/i.test(text),
        ipv4: (text) => /^(?:(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)\.){3}(?:25[0-5]|2[0-4]\d|1\d\d|[1-9]?\d)$/.test(text),
        ipv6,
        regex: (text) => {
            try { new RegExp(text, 'u'); return true; } catch { return false; }
        },
        uuid: (text) => /^(?:urn:uuid:)?[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}$/i.test(text),
        'json-pointer': (text) => /^(?:\/(?:[^~/]|~0|~1)*)*$/.test(text),
        'json-pointer-uri-fragment': (text) => /^#(?:\/(?:[a-z0-9_\-.!$&'()*+,;:=@]|%[0-9a-f]{2}|~0|~1)*)*$/i.test(text),
        'relative-json-pointer': (text) => /^(?:0|[1-9][0-9]*)(?:#|(?:\/(?:[^~/]|~0|~1)*)*)$/.test(text),
        byte: (text) => /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(text),
        password: () => true,
        binary: () => true,
    };
    const NUMBER_FORMATS = {
        int32: (n) => Number.isInteger(n) && n <= 2147483647 && n >= -2147483648,
        int64: (n) => Number.isInteger(n),
        float: () => true,
        double: () => true,
    };

    const typeOf = (value) => {
        if (value === null) return 'null';
        if (Array.isArray(value)) return 'array';
        return typeof value;
    };
    const isType = (value, type) => {
        switch (type) {
            case 'integer': return Number.isInteger(value);
            case 'number': return typeof value === 'number' && Number.isFinite(value);
            default: return typeOf(value) === type;
        }
    };
    const equal = (a, b) => {
        if (a === b) return true;
        if (typeOf(a) !== typeOf(b) || typeof a !== 'object' || a === null) return false;
        const keys = Object.keys(a);
        return keys.length === Object.keys(b).length
            && keys.every((key) => Object.prototype.hasOwnProperty.call(b, key) && equal(a[key], b[key]));
    };
    // `value` as `type`, as ajv's `coerceTypes` makes it; `undefined` when
    // it cannot be.
    const coerce = (value, type) => {
        const kind = typeOf(value);
        switch (type) {
            case 'string':
                if (kind === 'number' || kind === 'boolean') return String(value);
                if (kind === 'null') return '';
                return undefined;
            case 'number':
            case 'integer': {
                let number;
                if (kind === 'boolean') number = value ? 1 : 0;
                else if (kind === 'null') number = 0;
                else if (kind === 'string' && value.trim() !== '' && !Number.isNaN(Number(value))) number = Number(value);
                return number !== undefined && isType(number, type) ? number : undefined;
            }
            case 'boolean':
                if (value === 'true' || value === 1) return true;
                if (value === 'false' || value === 0 || value === null) return false;
                return undefined;
            case 'null':
                return value === '' || value === 0 || value === false ? null : undefined;
            default:
                return undefined;
        }
    };

    // The schema, checked as ajv compiles it.
    const check = (schema, path, options) => {
        if (typeof schema === 'boolean') return;
        if (typeOf(schema) !== 'object') throw new Error(`schema ${path} must be an object or a boolean`);
        for (const [keyword, value] of Object.entries(schema)) {
            if (!KEYWORDS.has(keyword)) {
                if (options.strict !== false) throw new Error(`strict mode: unknown keyword: "${keyword}"`);
                continue;
            }
            const at = `${path}/${keyword}`;
            if (keyword === 'type') {
                const types = Array.isArray(value) ? value : [value];
                for (const type of types) {
                    if (!TYPES.includes(type)) throw new Error(`schema is invalid: data${at} must be equal to one of the allowed values`);
                }
            } else if (keyword === 'format') {
                if (!(value in FORMATS) && !(value in NUMBER_FORMATS) && options.strict !== false) {
                    throw new Error(`unknown format "${value}" ignored in schema at path "#${path}"`);
                }
            } else if (keyword === 'pattern') {
                try { new RegExp(value, 'u'); } catch (e) { throw new Error(`schema is invalid: ${at}: ${e.message}`); }
            } else if (keyword === 'required' && !Array.isArray(value)) {
                throw new Error(`schema is invalid: data${at} must be array`);
            } else if (ONE.includes(keyword) && typeof value !== 'boolean') {
                check(value, at, options);
            } else if (LIST.includes(keyword)) {
                if (!Array.isArray(value)) throw new Error(`schema is invalid: data${at} must be array`);
                value.forEach((sub, i) => check(sub, `${at}/${i}`, options));
            } else if (MAP.includes(keyword)) {
                for (const [name, sub] of Object.entries(value ?? {})) check(sub, `${at}/${name}`, options);
            } else if (keyword === 'items') {
                const subs = Array.isArray(value) ? value : [value];
                subs.forEach((sub, i) => check(sub, Array.isArray(value) ? `${at}/${i}` : at, options));
            }
        }
    };

    const resolve = (root, ref) => {
        if (ref === '#') return root;
        if (!ref.startsWith('#/')) throw new Error(`can't resolve reference ${ref}`);
        let current = root;
        for (const raw of ref.slice(2).split('/')) {
            const key = decodeURIComponent(raw).replace(/~1/g, '/').replace(/~0/g, '~');
            current = current?.[key];
        }
        if (current === undefined) throw new Error(`can't resolve reference ${ref}`);
        return current;
    };

    const codePoints = (text) => [...text].length;

    // Validates `data` (at `path`) against `schema`, adding to `errors`.
    // Returns the value as coerced, for the parent to keep.
    const validate = (schema, data, path, schemaPath, ctx) => {
        const fail = (keyword, params, message) => {
            ctx.errors.push({ instancePath: path, schemaPath: `${schemaPath}/${keyword}`, keyword, params, message });
        };
        if (schema === true) return data;
        if (schema === false) {
            fail('false schema', {}, 'boolean schema is false');
            return data;
        }
        if (schema.$ref !== undefined) {
            return validate(resolve(ctx.root, schema.$ref), data, path, schema.$ref, ctx);
        }
        let value = data;
        if (schema.type !== undefined) {
            const types = Array.isArray(schema.type) ? schema.type : [schema.type];
            if (!types.some((type) => isType(value, type))) {
                const coerced = ctx.coerce
                    ? types.map((type) => coerce(value, type)).find((v) => v !== undefined)
                    : undefined;
                if (coerced === undefined) {
                    fail('type', { type: types.join(',') }, `must be ${types.join(',')}`);
                    return value;
                }
                value = coerced;
            }
        }
        if (schema.enum !== undefined && !schema.enum.some((option) => equal(option, value))) {
            fail('enum', { allowedValues: schema.enum }, 'must be equal to one of the allowed values');
        }
        if ('const' in schema && !equal(schema.const, value)) {
            fail('const', { allowedValue: schema.const }, 'must be equal to constant');
        }
        const kind = typeOf(value);
        if (kind === 'number') numeric(schema, value, fail);
        if (kind === 'string') textual(schema, value, fail);
        if (kind === 'array') list(schema, value, path, schemaPath, ctx, fail);
        if (kind === 'object') object(schema, value, path, schemaPath, ctx, fail);
        combine(schema, value, path, schemaPath, ctx, fail);
        return value;
    };

    const numeric = (schema, value, fail) => {
        const limit = (keyword, ok, comparison) => {
            if (schema[keyword] !== undefined && !ok(schema[keyword])) {
                fail(keyword, { comparison, limit: schema[keyword] }, `must be ${comparison} ${schema[keyword]}`);
            }
        };
        limit('minimum', (n) => value >= n, '>=');
        limit('maximum', (n) => value <= n, '<=');
        limit('exclusiveMinimum', (n) => value > n, '>');
        limit('exclusiveMaximum', (n) => value < n, '<');
        if (schema.multipleOf !== undefined) {
            const ratio = value / schema.multipleOf;
            if (Math.abs(ratio - Math.round(ratio)) > 1e-9) {
                fail('multipleOf', { multipleOf: schema.multipleOf }, `must be multiple of ${schema.multipleOf}`);
            }
        }
        const format = NUMBER_FORMATS[schema.format];
        if (format && !format(value)) fail('format', { format: schema.format }, `must match format "${schema.format}"`);
    };

    const textual = (schema, value, fail) => {
        if (schema.minLength !== undefined && codePoints(value) < schema.minLength) {
            fail('minLength', { limit: schema.minLength }, `must NOT have fewer than ${schema.minLength} characters`);
        }
        if (schema.maxLength !== undefined && codePoints(value) > schema.maxLength) {
            fail('maxLength', { limit: schema.maxLength }, `must NOT have more than ${schema.maxLength} characters`);
        }
        if (schema.pattern !== undefined && !new RegExp(schema.pattern, 'u').test(value)) {
            fail('pattern', { pattern: schema.pattern }, `must match pattern "${schema.pattern}"`);
        }
        const format = FORMATS[schema.format];
        if (format && !format(value)) fail('format', { format: schema.format }, `must match format "${schema.format}"`);
    };

    const list = (schema, value, path, schemaPath, ctx, fail) => {
        const { items } = schema;
        if (Array.isArray(items)) {
            value.forEach((item, i) => {
                const sub = i < items.length ? items[i] : schema.additionalItems;
                const where = i < items.length ? `${schemaPath}/items/${i}` : `${schemaPath}/additionalItems`;
                if (sub !== undefined) value[i] = validate(sub, item, `${path}/${i}`, where, ctx);
            });
        } else if (items !== undefined) {
            value.forEach((item, i) => { value[i] = validate(items, item, `${path}/${i}`, `${schemaPath}/items`, ctx); });
        }
        if (schema.minItems !== undefined && value.length < schema.minItems) {
            fail('minItems', { limit: schema.minItems }, `must NOT have fewer than ${schema.minItems} items`);
        }
        if (schema.maxItems !== undefined && value.length > schema.maxItems) {
            fail('maxItems', { limit: schema.maxItems }, `must NOT have more than ${schema.maxItems} items`);
        }
        if (schema.uniqueItems === true) {
            for (let i = 0; i < value.length; i++) {
                const j = value.findIndex((other, k) => k > i && equal(other, value[i]));
                if (j >= 0) {
                    fail('uniqueItems', { i, j }, `must NOT have duplicate items (items ## ${j} and ${i} are identical)`);
                    break;
                }
            }
        }
        if (schema.contains !== undefined && !value.some((item, i) => passes(schema.contains, item, `${path}/${i}`, ctx))) {
            fail('contains', {}, 'must contain at least 1 valid item(s)');
        }
    };

    const object = (schema, value, path, schemaPath, ctx, fail) => {
        const keys = Object.keys(value);
        for (const name of schema.required ?? []) {
            if (!Object.prototype.hasOwnProperty.call(value, name)) {
                fail('required', { missingProperty: name }, `must have required property '${name}'`);
            }
        }
        if (schema.minProperties !== undefined && keys.length < schema.minProperties) {
            fail('minProperties', { limit: schema.minProperties }, `must NOT have fewer than ${schema.minProperties} properties`);
        }
        if (schema.maxProperties !== undefined && keys.length > schema.maxProperties) {
            fail('maxProperties', { limit: schema.maxProperties }, `must NOT have more than ${schema.maxProperties} properties`);
        }
        const properties = schema.properties ?? {};
        const patterns = Object.entries(schema.patternProperties ?? {}).map(([p, sub]) => [new RegExp(p, 'u'), p, sub]);
        for (const key of keys) {
            let matched = false;
            if (Object.prototype.hasOwnProperty.call(properties, key)) {
                matched = true;
                value[key] = validate(properties[key], value[key], `${path}/${key}`, `${schemaPath}/properties/${key}`, ctx);
            }
            for (const [regex, source, sub] of patterns) {
                if (regex.test(key)) {
                    matched = true;
                    value[key] = validate(sub, value[key], `${path}/${key}`, `${schemaPath}/patternProperties/${source}`, ctx);
                }
            }
            if (!matched && schema.additionalProperties !== undefined) {
                if (schema.additionalProperties === false) {
                    fail('additionalProperties', { additionalProperty: key }, 'must NOT have additional properties');
                } else {
                    value[key] = validate(schema.additionalProperties, value[key], `${path}/${key}`, `${schemaPath}/additionalProperties`, ctx);
                }
            }
            if (schema.propertyNames !== undefined && !passes(schema.propertyNames, key, path, ctx)) {
                fail('propertyNames', { propertyName: key }, 'property name must be valid');
            }
        }
        for (const [name, dependency] of Object.entries(schema.dependencies ?? {})) {
            if (!Object.prototype.hasOwnProperty.call(value, name)) continue;
            if (Array.isArray(dependency)) {
                for (const needed of dependency) {
                    if (!Object.prototype.hasOwnProperty.call(value, needed)) {
                        fail('dependencies', { property: name, missingProperty: needed }, `must have property ${needed} when property ${name} is present`);
                    }
                }
            } else {
                validate(dependency, value, path, `${schemaPath}/dependencies/${name}`, ctx);
            }
        }
    };

    const combine = (schema, value, path, schemaPath, ctx, fail) => {
        (schema.allOf ?? []).forEach((sub, i) => validate(sub, value, path, `${schemaPath}/allOf/${i}`, ctx));
        if (schema.anyOf !== undefined && !schema.anyOf.some((sub) => passes(sub, value, path, ctx))) {
            fail('anyOf', {}, 'must match a schema in anyOf');
        }
        if (schema.oneOf !== undefined) {
            const matching = schema.oneOf.filter((sub) => passes(sub, value, path, ctx)).length;
            if (matching !== 1) fail('oneOf', { passingSchemas: matching }, 'must match exactly one schema in oneOf');
        }
        if (schema.not !== undefined && passes(schema.not, value, path, ctx)) {
            fail('not', {}, 'must NOT be valid');
        }
        if (schema.if !== undefined) {
            const branch = passes(schema.if, value, path, ctx) ? 'then' : 'else';
            if (schema[branch] !== undefined && !passes(schema[branch], value, path, ctx)) {
                fail('if', { failingKeyword: branch }, `must match "${branch}" schema`);
            }
        }
    };

    // Whether `data` is valid against `schema`, without reporting why.
    const passes = (schema, data, path, ctx) => {
        const errors = [];
        validate(schema, structuredCopy(data), path, '#', { ...ctx, errors });
        return errors.length === 0;
    };
    const structuredCopy = (value) => (value === undefined ? undefined : JSON.parse(JSON.stringify(value)));

    // Compiles `schema` with ajv's `options`: throws if it is not valid,
    // and gives a function that returns the errors of a value (none: `[]`).
    const compile = (schema, options = {}) => {
        check(schema, '', options);
        return (data) => {
            const ctx = { root: schema, coerce: Boolean(options.coerceTypes), errors: [] };
            // Coercion changes the value, as in ajv; a copy is checked.
            validate(schema, structuredCopy(data), '', '#', ctx);
            return ctx.errors;
        };
    };

    globalThis.__apifast_schema = { compile };
})();

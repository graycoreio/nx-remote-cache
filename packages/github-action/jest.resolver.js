// @actions/* v3 packages are ESM-only (they only export an `import` condition),
// so resolve them through that condition and let ts-jest transpile them to CommonJS.
module.exports = (request, options) =>
    options.defaultResolver(request, request.startsWith('@actions/')
        ? { ...options, conditions: ['import', 'node', 'default'] }
        : options);

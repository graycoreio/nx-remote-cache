module.exports = {
    clearMocks: true,
    moduleFileExtensions: ['js', 'ts'],
    testMatch: ['**/*.spec.ts'],
    resolver: '<rootDir>/jest.resolver.js',
    transformIgnorePatterns: ['/node_modules/(?!@actions/)'],
    transform: {
        '^.+\\.[jt]s$': ['ts-jest', { compiler: '@typescript/typescript6', tsconfig: { module: 'commonjs', moduleResolution: 'bundler', allowJs: true, esModuleInterop: true, types: ['node', 'jest'], strict: true } }]
    },
    verbose: true
}

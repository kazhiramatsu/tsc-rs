declare class UnsetAdditionalOptions { _UnsetAdditionalOptions: true; }
export declare class Hook<T, R, AdditionalOptions = UnsetAdditionalOptions> { name: string | undefined; call(...args: T extends unknown[] ? T : never): R; }
export declare class SyncBailHook<T, R, AdditionalOptions = UnsetAdditionalOptions> extends Hook<T, R, AdditionalOptions> {}
type AnyHook = Hook<any, any>;
export declare class TypedHookMap<M extends Record<any, AnyHook>> {
	get<K extends keyof M>(key: K): M[K] | undefined;
	for<K extends keyof M>(key: K): M[K];
}

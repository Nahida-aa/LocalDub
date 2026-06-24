declare module 'onnxruntime-node' {
  export class InferenceSession {
    static create(path: string, options?: any): Promise<InferenceSession>;
    inputNames?: string[];
    outputNames?: string[];
    run(feed: Record<string, any>): Promise<Record<string, any>>;
    release(): Promise<void>;
  }
  export class Tensor {
    constructor(dtype: string, data: any, dims: number[]);
    dims?: number[];
  }
  export function listSupportedBackends(): string[];
  export type Ort = any;
}

declare module 'onnxruntime-node-gpu' {
  export * from 'onnxruntime-node';
}


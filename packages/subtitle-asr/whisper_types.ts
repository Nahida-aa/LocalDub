export type WhisperJsonSegment = {
  timestamps: {
    from: string; // "00:00:00,030"
    to: string; // "00:00:01,030"
  };
  offsets: {
    from: number; // 30
    to: number; // 1030
  };
  text: string; // "一群夜处"
  tokens: [
    {
      text: string; // "[_BEG_]"
      timestamps: {
        from: string; // "00:00:00,000"
        to: string; // "00:00:00,000"
      };
      offsets: {
        from: number; // 0
        to: number; // 0
      };
      id: number;
      p: number; // 0.862423
      t_dtw: number; // -1
    },
    {
      text: string; // "一"
      timestamps: {
        from: string; // "00:00:00,000"
        to: string; // "00:00:00,210"
      };
      offsets: {
        from: number; // 0
        to: number; // 210
      };
      id: number;
      p: number; // 0.530553
      t_dtw: number; // -1
    },
    {
      text: string; // "群"
      timestamps: {
        from: string; // "00:00:00,280"
        to: string; // "00:00:00,460"
      };
      offsets: {
        from: number; // 280
        to: number; // 460
      };
      id: number;
      p: number; // 0.997099
      t_dtw: number; // -1
    },
    {
      text: string; // "夜"
      timestamps: {
        from: string; // "00:00:00,510"
        to: string; // "00:00:00,740"
      };
      offsets: {
        from: number; // 510
        to: number; // 740
      };
      id: number;
      p: number; // 0.763127
      t_dtw: number; // -1
    },
    {
      text: string; // "处"
      timestamps: {
        from: string; // "00:00:00,740"
        to: string; // "00:00:00,990"
      };
      offsets: {
        from: number; // 740
        to: number; // 990
      };
      id: number;
      p: number; // 0.281056
      t_dtw: number; // -1
    },
    {
      text: string; // "[_TT_50]"
      timestamps: {
        from: string; // "00:00:01,000"
        to: string; // "00:00:01,000"
      };
      offsets: {
        from: number; // 1000
        to: number; // 1000
      };
      id: number;
      p: number; // 0.025546
      t_dtw: number; // -1
    },
  ];
};

export type WhisperJson = {
  systeminfo: string; // "WHISPER : COREML = 0 | OPENVINO = 0 | CPU : SSE3 = 1 | SSSE3 = 1 | AVX = 1 | AVX2 = 1 | F16C = 1 | FMA = 1 | BMI2 = 1 | AVX512 = 1 | AVX512_VBMI = 1 | AVX512_VNNI = 1 | AVX512_BF16 = 1 | OPENMP = 1 | REPACK = 1 | "
  model: {
    type: string; // large
    multilingual: boolean;
    vocab: number;
    audio: {
      ctx: number;
      state: number;
      head: number;
      layer: number;
    };
    text: {
      ctx: number;
      state: number;
      head: number;
      layer: number;
    };
    mels: number;
    ftype: number;
  };
  params: {
    model: string; // "/home/aa/repos/env_ls/LocalDub/data/models/whisper/ggml-large-v3-turbo.bin"
    language: string; // "auto"
    translate: boolean;
  };
  result: {
    language: string; // "zh"
  };
  transcription: WhisperJsonSegment[];
};

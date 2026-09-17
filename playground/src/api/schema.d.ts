/**
 * Generated OpenAPI client schema snapshot.
 * Regenerate against a running KoeServe with: npm run generate:api
 */
export interface paths {
  "/health": {
    get: operations["health"];
  };
  "/v1/voices": {
    get: operations["list_voices"];
  };
  "/v1/speech": {
    post: operations["synthesize"];
  };
}

export interface components {
  schemas: {
    SynthesisRequest: {
      text: string;
      voice: string;
      speed?: number | null;
      pitch?: number | null;
      alpha?: number | null;
      beta?: number | null;
      volume_db?: number | null;
    };
    VoiceDefaults: {
      sampling_frequency: number;
      frame_period: number;
      speed: number;
      pitch: number;
      alpha: number;
      beta: number;
      volume_db: number;
    };
    VoiceInfo: {
      id: string;
      speaker: string;
      style: string;
      display_name: string;
      defaults: components["schemas"]["VoiceDefaults"];
    };
    ApiErrorBody: {
      code: string;
      message: string;
    };
  };
}

export interface operations {
  health: {
    responses: {
      200: { content: { "application/json": unknown } };
    };
  };
  list_voices: {
    responses: {
      200: { content: { "application/json": components["schemas"]["VoiceInfo"][] } };
    };
  };
  synthesize: {
    requestBody: {
      content: { "application/json": components["schemas"]["SynthesisRequest"] };
    };
    responses: {
      200: { content: { "audio/wav": Blob } };
      400: { content: { "application/json": components["schemas"]["ApiErrorBody"] } };
      500: { content: { "application/json": components["schemas"]["ApiErrorBody"] } };
      503: { content: { "application/json": components["schemas"]["ApiErrorBody"] } };
    };
  };
}

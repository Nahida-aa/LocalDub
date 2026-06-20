import { test, expect } from 'bun:test';
import { normalizeOpenAIBaseUrl } from './url.ts';

test('normalizeOpenAIBaseUrl', () => {
	expect(normalizeOpenAIBaseUrl(undefined)).toBe('https://api.openai.com/v1');
	expect(normalizeOpenAIBaseUrl('')).toBe('https://api.openai.com/v1');
	expect(normalizeOpenAIBaseUrl('   ')).toBe('https://api.openai.com/v1');

	expect(normalizeOpenAIBaseUrl('https://api.openai.com/v1/')).toBe('https://api.openai.com/v1');
	expect(normalizeOpenAIBaseUrl('https://api.openai.com/v1///')).toBe('https://api.openai.com/v1');

	expect(normalizeOpenAIBaseUrl('https://api.openai.com/v1/chat/completions')).toBe('https://api.openai.com/v1');
	expect(normalizeOpenAIBaseUrl('https://api.openai.com/v1/completions')).toBe('https://api.openai.com/v1');

	expect(normalizeOpenAIBaseUrl('https://my-proxy.com/v1')).toBe('https://my-proxy.com/v1');

	expect(normalizeOpenAIBaseUrl('https://API.OpenAI.com/v1/CHAT/COMPLETIONS')).toBe('https://API.OpenAI.com/v1');

	expect(normalizeOpenAIBaseUrl('/chat/completions')).toBe('https://api.openai.com/v1');

	expect(normalizeOpenAIBaseUrl('https://my.com/v1/chat/completions/')).toBe('https://my.com/v1');
});
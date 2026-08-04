import { copyFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { basename, join } from 'node:path';
import { DUB_STAGES, getStages } from '@repo/core/stages/utils/stages';
import { existsSync } from '@repo/core/utils/fileOps';
import { InputArgs } from '@repo/core/input/input';
import { runPipeline } from '../../tasks/pipeline-runner.ts';
import { downloadVideo, importVideo } from './import/download';
import { playTaskFail, playTaskSuccess } from '@repo/core/cmd/tasks/utils';


export const cmdStartTask = async (input: InputArgs) => {
	const args = input.task ?? {};
	const url = args.url
	if (!url) {
		console.error('task start: need task.url in input.json',);
		throw new Error('task start: need task.url in input.json');
	}
	const ctx = await importVideo(input);
	console.log(`\n[CLI] Running pipeline ...`);
	await runPipeline(ctx)
	console.log('[CLI] Pipeline success');
	playTaskSuccess()
}

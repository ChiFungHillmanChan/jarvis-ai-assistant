import { GetObjectCommand, S3Client } from '@aws-sdk/client-s3';
import { makeHandler } from './handler';

const s3 = new S3Client({});
const Bucket = process.env.REGISTRY_BUCKET ?? '';
const Key = 'registry/latest.json';

export const handler = makeHandler(async () => {
  const object = await s3.send(new GetObjectCommand({ Bucket, Key }));
  if (!object.Body) throw Object.assign(new Error('empty object'), { name: 'EmptyObject' });
  return { body: await object.Body.transformToByteArray(), signature: object.Metadata?.signature };
});

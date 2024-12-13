#!/usr/bin/env node

import * as cdk from 'aws-cdk-lib';
import { LambdaReadAsideStack } from '../lib/lambda-read-aside-stack';

const app = new cdk.App();
new LambdaReadAsideStack(app, 'LambdaReadAsideStack', {
    env: {
        region: "us-east-1"
    }
});

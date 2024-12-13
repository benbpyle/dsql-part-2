import { Effect, PolicyStatement } from "aws-cdk-lib/aws-iam";
import { Architecture, FunctionUrlAuthType, LayerVersion } from "aws-cdk-lib/aws-lambda";
import { RustFunction } from "cargo-lambda-cdk";
import { Construct } from "constructs"

export class LambdaConstruct extends Construct {
  constructor(scope: Construct, id: string) {
    super(scope, id);

    const layer = LayerVersion.fromLayerVersionArn(
      this,
      'DatadogExtension',
      'arn:aws:lambda:us-east-1:464622532012:layer:Datadog-Extension-ARM:67'
    )

    const select = new RustFunction(scope, 'SelectFunction', {
      architecture: Architecture.ARM_64,
      functionName: "cacheable-table-select",
      manifestPath: 'rust/get-lambda',
      memorySize: 256,
      environment: {
        CLUSTER_ENDPOINT: process.env.CLUSTER_ENDPOINT!,
        MOMENTO_API_KEY: process.env.MOMENTO_API_KEY!,
        CACHE_NAME: "CacheableTable",
        DD_SERVICE: 'get-lambda',
        DD_API_KEY: process.env.DD_API_KEY!,
        DD_SITE: process.env.DD_SITE!,
        RUST_LOG: 'info',
      },
      layers: [layer]
    })

    select.addToRolePolicy(new PolicyStatement({
      effect: Effect.ALLOW,
      actions: ["dsql:*"],
      resources: ["*"]
    }))

    select.addFunctionUrl({
      authType: FunctionUrlAuthType.NONE,
      cors: {
        allowedOrigins: ["*"]
      }
    })


  }
}

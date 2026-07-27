import { Strategy as SamlStrategy } from 'passport-saml';
import { PrismaClient } from '@prisma/client';

const prisma = new PrismaClient();

export interface SamlConfig {
    entryPoint: string;
    issuer: string;
    callbackUrl: string;
    cert: string;
    privateKey?: string;
}

export interface SCIMUser {
    emails: { value: string }[];
    name?: { formatted: string };
    active: boolean;
}

export class SSOService {
    // Configure SAML for enterprise customer
    configureSaml(tenantId: string, config: SamlConfig) {
        const strategy = new SamlStrategy(
            {
                entryPoint: config.entryPoint,
                issuer: config.issuer,
                callbackUrl: config.callbackUrl,
                cert: config.cert,
                decryptionPvk: config.privateKey,
                signatureAlgorithm: 'sha256',
                digestAlgorithm: 'sha256',
                identifierFormat: 'urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress',
            },
            async (profile: any, done: any) => {
                try {
                    const email = profile.nameID || profile.email;
                    const user = await prisma.user.upsert({
                        where: { email },
                        update: {
                            active: true,
                        },
                        create: {
                            email,
                            name: profile.displayName || profile.cn || email.split('@')[0],
                            plan: 'pro',
                        },
                    });

                    return done(null, user);
                } catch (error) {
                    return done(error);
                }
            }
        );

        return strategy;
    }

    // Generate SP metadata for customer
    generateMetadata(issuer: string, callbackUrl: string, cert: string): string {
        return `<?xml version="1.0"?>
<md:EntityDescriptor xmlns:md="urn:oasis:names:tc:SAML:2.0:metadata"
                     entityID="${issuer}">
  <md:SPSSODescriptor AuthnRequestsSigned="true"
                      WantAssertionsSigned="true"
                      protocolSupportEnumeration="urn:oasis:names:tc:SAML:2.0:protocol">
    <md:KeyDescriptor use="signing">
      <ds:KeyInfo xmlns:ds="http://www.w3.org/2000/09/xmldsig#">
        <ds:X509Data>
          <ds:X509Certificate>${cert}</ds:X509Certificate>
        </ds:X509Data>
      </ds:KeyInfo>
    </md:KeyDescriptor>
    <md:SingleLogoutService Binding="urn:oasis:names:tc:SAML:2.0:bindings:HTTP-Redirect"
                            Location="${callbackUrl.replace('/callback', '/logout')}"/>
    <md:NameIDFormat>urn:oasis:names:tc:SAML:1.1:nameid-format:emailAddress</md:NameIDFormat>
    <md:AssertionConsumerService Binding="urn:oasis:names:tc:SAML:2.0:bindings:HTTP-POST"
                                 Location="${callbackUrl}"
                                 index="1"/>
  </md:SPSSODescriptor>
</md:EntityDescriptor>`;
    }

    // SCIM provisioning for user management
    async provisionUser(tenantId: string, userData: SCIMUser) {
        return await prisma.user.upsert({
            where: { email: userData.emails[0].value },
            update: {
                name: userData.name?.formatted,
                active: userData.active,
            },
            create: {
                email: userData.emails[0].value,
                name: userData.name?.formatted || userData.emails[0].value.split('@')[0],
                plan: 'pro',
            },
        });
    }

    // Deactivate user via SCIM
    async deactivateUser(userId: string) {
        return await prisma.user.update({
            where: { id: userId },
            data: { active: false },
        });
    }
}

export const ssoService = new SSOService();

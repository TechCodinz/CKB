import crypto from 'crypto';
import { PrismaClient } from '@prisma/client';
import { auditService, AuditAction } from './audit-log';

const prisma = new PrismaClient();

export class ApiKeyService {
    // Generate new API key
    async createKey(
        userId: string,
        name: string,
        permissions: string[] = ['read'],
        expiresInDays?: number
    ): Promise<{ key: string; id: string }> {
        const rawKey = crypto.randomBytes(32).toString('hex');
        const prefix = rawKey.slice(0, 8);

        const hash = crypto
            .createHash('sha256')
            .update(rawKey)
            .digest('hex');

        const apiKey = await prisma.apiKey.create({
            data: {
                userId,
                name,
                prefix,
                hash,
                permissions: JSON.stringify(permissions),
                expiresAt: expiresInDays
                    ? new Date(Date.now() + expiresInDays * 24 * 60 * 60 * 1000)
                    : undefined,
            },
        });

        await auditService.log({
            tenantId: 'default',
            userId,
            action: AuditAction.API_KEY_CREATED,
            resourceType: 'api_key',
            resourceId: apiKey.id,
            details: { name, permissions, expiresInDays },
            timestamp: new Date(),
        });

        return { key: rawKey, id: apiKey.id };
    }

    // Validate API key
    async validateKey(rawKey: string) {
        const hash = crypto.createHash('sha256').update(rawKey).digest('hex');

        const key = await prisma.apiKey.findUnique({
            where: { hash },
        });

        if (!key) return { valid: false };
        if (key.expiresAt && key.expiresAt < new Date()) return { valid: false };
        if (!key.active) return { valid: false };

        await prisma.apiKey.update({
            where: { id: key.id },
            data: { lastUsedAt: new Date() },
        });

        return { valid: true, key };
    }

    // List keys for user
    async listKeys(userId: string) {
        return await prisma.apiKey.findMany({
            where: { userId },
            select: {
                id: true,
                name: true,
                prefix: true,
                permissions: true,
                expiresAt: true,
                lastUsedAt: true,
                createdAt: true,
                active: true,
            },
            orderBy: { createdAt: 'desc' },
        });
    }

    // Revoke API key
    async revokeKey(keyId: string, userId: string) {
        const key = await prisma.apiKey.findFirst({
            where: { id: keyId, userId },
        });

        if (!key) throw new Error('Key not found');

        await prisma.apiKey.update({
            where: { id: keyId },
            data: { active: false },
        });

        await auditService.log({
            tenantId: 'default',
            userId,
            action: AuditAction.API_KEY_REVOKED,
            resourceType: 'api_key',
            resourceId: keyId,
            details: { name: key.name },
            timestamp: new Date(),
        });

        return { success: true };
    }
}

export const apiKeyService = new ApiKeyService();

import crypto from 'crypto';
import { PrismaClient } from '@prisma/client';
import { auditService, AuditAction } from './audit-log';

const prisma = new PrismaClient();

export interface ApiKey {
    id: string;
    tenantId: string;
    userId: string;
    name: string;
    prefix: string;
    hash: string;
    permissions: string[];
    expiresAt?: Date;
    lastUsedAt?: Date;
    createdAt: Date;
    createdBy: string;
}

export class ApiKeyService {
    // Generate new API key
    async createKey(
        tenantId: string,
        userId: string,
        name: string,
        permissions: string[] = ['read'],
        expiresInDays?: number
    ): Promise<{ key: string; id: string }> {
        // Generate random key
        const rawKey = crypto.randomBytes(32).toString('hex');
        const prefix = rawKey.slice(0, 8);

        // Hash for storage
        const hash = crypto
            .createHash('sha256')
            .update(rawKey)
            .digest('hex');

        // Store in database
        const apiKey = await prisma.apiKey.create({
            data: {
                tenantId,
                userId,
                name,
                prefix,
                hash,
                permissions,
                expiresAt: expiresInDays
                    ? new Date(Date.now() + expiresInDays * 24 * 60 * 60 * 1000)
                    : undefined,
                createdBy: userId,
            },
        });

        // Audit log
        await auditService.log({
            tenantId,
            userId,
            action: AuditAction.API_KEY_CREATED,
            resourceType: 'api_key',
            resourceId: apiKey.id,
            details: { name, permissions, expiresInDays },
            timestamp: new Date(),
        });

        // Return the raw key (only time it's visible)
        return { key: rawKey, id: apiKey.id };
    }

    // Validate API key
    async validateKey(rawKey: string): Promise<{ valid: boolean; key?: ApiKey }> {
        const hash = crypto.createHash('sha256').update(rawKey).digest('hex');

        const key = await prisma.apiKey.findFirst({
            where: { hash },
            include: { tenant: true },
        } as any); // using as any since apiKey relates to Prisma schema we haven't seen in full

        if (!key) return { valid: false };

        // Check expiration
        if (key.expiresAt && key.expiresAt < new Date()) {
            return { valid: false };
        }

        // Check if active
        if (!key.active) return { valid: false };

        // Update last used
        await prisma.apiKey.update({
            where: { id: key.id },
            data: { lastUsedAt: new Date() },
        });

        return { valid: true, key: key as any };
    }

    // List keys for tenant
    async listKeys(tenantId: string, userId?: string) {
        const where: any = { tenantId };
        if (userId) where.userId = userId;

        return await prisma.apiKey.findMany({
            where,
            select: {
                id: true,
                name: true,
                prefix: true,
                permissions: true,
                expiresAt: true,
                lastUsedAt: true,
                createdAt: true,
                createdBy: true,
                active: true,
            },
            orderBy: { createdAt: 'desc' },
        });
    }

    // Revoke API key
    async revokeKey(tenantId: string, keyId: string, userId: string) {
        const key = await prisma.apiKey.findFirst({
            where: { id: keyId, tenantId },
        });

        if (!key) throw new Error('Key not found');

        await prisma.apiKey.update({
            where: { id: keyId },
            data: { active: false },
        });

        await auditService.log({
            tenantId,
            userId,
            action: AuditAction.API_KEY_REVOKED,
            resourceType: 'api_key',
            resourceId: keyId,
            details: { name: key.name },
            timestamp: new Date(),
        });

        return { success: true };
    }

    // Update key permissions
    async updatePermissions(tenantId: string, keyId: string, permissions: string[], userId: string) {
        const key = await prisma.apiKey.findFirst({
            where: { id: keyId, tenantId },
        });

        if (!key) throw new Error('Key not found');

        await prisma.apiKey.update({
            where: { id: keyId },
            data: { permissions },
        });

        await auditService.log({
            tenantId,
            userId,
            action: AuditAction.API_KEY_UPDATED,
            resourceType: 'api_key',
            resourceId: keyId,
            details: { name: key.name, oldPermissions: key.permissions as any, newPermissions: permissions },
            timestamp: new Date(),
        });

        return { success: true };
    }
}

export const apiKeyService = new ApiKeyService();

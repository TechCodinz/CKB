import express from 'express';
import cors from 'cors';
import helmet from 'helmet';
import rateLimit from 'express-rate-limit';
import { PrismaClient } from '@prisma/client';
import bcrypt from 'bcryptjs';
import jwt from 'jsonwebtoken';

const app = express();
const prisma = new PrismaClient();
const PORT = process.env.PORT || 4000;
const JWT_SECRET = process.env.JWT_SECRET || 'ckb-dev-secret-change-in-production';

// ─── Middleware ───
app.use(helmet());
app.use(cors({
    origin: process.env.FRONTEND_URL || 'http://localhost:3001',
    credentials: true,
}));
app.use(express.json());
app.use(rateLimit({ windowMs: 15 * 60 * 1000, max: 100, message: 'Too many requests' }));

// ─── Auth Middleware ───
function authenticate(req: any, res: any, next: any) {
    const auth = req.headers.authorization;
    if (!auth?.startsWith('Bearer ')) return res.status(401).json({ message: 'Unauthorized' });
    try {
        req.user = jwt.verify(auth.split(' ')[1], JWT_SECRET);
        next();
    } catch {
        return res.status(401).json({ message: 'Invalid token' });
    }
}

// ─── Health ───
app.get('/health', (_req, res) => res.json({ status: 'ok', version: '1.0.0' }));

// ─── Auth Routes ───
app.post('/api/v1/auth/register', async (req, res) => {
    const { email, password, name } = req.body;
    if (!email || !password || !name) {
        return res.status(400).json({ message: 'Email, password, and name are required' });
    }
    if (password.length < 8) {
        return res.status(400).json({ message: 'Password must be at least 8 characters' });
    }
    try {
        const exists = await prisma.user.findUnique({ where: { email } });
        if (exists) return res.status(409).json({ message: 'Email already registered' });

        const hash = await bcrypt.hash(password, 12);
        const user = await prisma.user.create({
            data: { email, name, passwordHash: hash, plan: 'free' },
        });

        const token = jwt.sign({ id: user.id, email: user.email, plan: user.plan }, JWT_SECRET, { expiresIn: '30d' });
        return res.status(201).json({ token, user: { id: user.id, email: user.email, name: user.name, plan: user.plan } });
    } catch (err: any) {
        console.error('Register error:', err);
        return res.status(500).json({ message: 'Registration failed' });
    }
});

app.post('/api/v1/auth/login', async (req, res) => {
    const { email, password } = req.body;
    if (!email || !password) return res.status(400).json({ message: 'Email and password required' });

    try {
        const user = await prisma.user.findUnique({ where: { email } });
        if (!user || !user.passwordHash) return res.status(401).json({ message: 'Invalid credentials' });

        const valid = await bcrypt.compare(password, user.passwordHash);
        if (!valid) return res.status(401).json({ message: 'Invalid credentials' });

        const token = jwt.sign({ id: user.id, email: user.email, plan: user.plan }, JWT_SECRET, { expiresIn: '30d' });
        return res.json({ token, user: { id: user.id, email: user.email, name: user.name, plan: user.plan } });
    } catch (err) {
        console.error('Login error:', err);
        return res.status(500).json({ message: 'Login failed' });
    }
});

app.get('/api/v1/auth/me', authenticate, async (req: any, res) => {
    const user = await prisma.user.findUnique({ where: { id: req.user.id } });
    if (!user) return res.status(404).json({ message: 'User not found' });
    return res.json({ id: user.id, email: user.email, name: user.name, plan: user.plan });
});

// ─── Projects ───
app.get('/api/v1/projects', authenticate, async (req: any, res) => {
    const projects = await prisma.project.findMany({ where: { userId: req.user.id } });
    return res.json(projects);
});

app.post('/api/v1/projects', authenticate, async (req: any, res) => {
    const { name, path } = req.body;
    const project = await prisma.project.create({
        data: { name, path, userId: req.user.id },
    });
    return res.status(201).json(project);
});

// ─── Billing ───
app.get('/api/v1/billing/subscription', authenticate, async (req: any, res) => {
    const user = await prisma.user.findUnique({ where: { id: req.user.id } });
    return res.json({ plan: user?.plan || 'free' });
});

app.post('/api/v1/billing/checkout', authenticate, async (req: any, res) => {
    const { plan } = req.body;
    // Return Coinbase Commerce checkout URL based on plan
    const links: Record<string, string> = {
        'pro': process.env.COINBASE_PRO_LINK || 'https://commerce.coinbase.com/checkout/YOUR_PRO_LINK',
        'team': process.env.COINBASE_TEAM_LINK || 'https://commerce.coinbase.com/checkout/YOUR_TEAM_LINK',
    };
    const url = links[plan];
    if (!url) return res.status(400).json({ message: 'Invalid plan' });
    return res.json({ url });
});

// ─── Start ───
app.listen(PORT, () => {
    console.log(`CKB Backend running on port ${PORT}`);
});

export default app;

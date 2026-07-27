import React, { useState, useContext } from 'react';
import { useNavigate, Link } from 'react-router-dom';
import {
    Box, Card, CardContent, Typography, TextField,
    Button, CircularProgress, Alert
} from '@mui/material';
import { AuthContext } from '../App';
import { ckbApi } from '../services/api';

export default function Signup() {
    const navigate = useNavigate();
    const { login } = useContext(AuthContext);
    const [name, setName] = useState('');
    const [email, setEmail] = useState('');
    const [password, setPassword] = useState('');
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState('');

    const handleSubmit = async (e: React.FormEvent) => {
        e.preventDefault();
        if (password.length < 8) {
            setError('Password must be at least 8 characters');
            return;
        }
        setLoading(true);
        setError('');
        try {
            const resp = await ckbApi.register(email, password, name);
            const { token } = resp.data;
            localStorage.setItem('ckb_token', token);
            login();
            navigate('/');
        } catch (err: any) {
            setError(err?.response?.data?.message || 'Registration failed. Try again.');
        } finally {
            setLoading(false);
        }
    };

    return (
        <Box sx={{
            minHeight: '100vh',
            background: 'linear-gradient(135deg, #0a0a1a 0%, #1a0a2e 50%, #0a1a2e 100%)',
            display: 'flex', alignItems: 'center', justifyContent: 'center',
        }}>
            <Card sx={{
                width: '100%', maxWidth: 440,
                background: 'rgba(255,255,255,0.05)',
                backdropFilter: 'blur(20px)',
                border: '1px solid rgba(255,255,255,0.1)',
                borderRadius: 3,
            }}>
                <CardContent sx={{ p: 4 }}>
                    <Box sx={{ textAlign: 'center', mb: 4 }}>
                        <Typography variant="h4" sx={{
                            fontWeight: 800,
                            background: 'linear-gradient(90deg, #90caf9 0%, #ce93d8 100%)',
                            WebkitBackgroundClip: 'text', WebkitTextFillColor: 'transparent'
                        }}>
                            CKB
                        </Typography>
                        <Typography color="textSecondary" variant="body2">14-day free trial — no credit card required</Typography>
                    </Box>

                    <Typography variant="h6" fontWeight={600} mb={3}>Create your account</Typography>

                    {error && <Alert severity="error" sx={{ mb: 2 }}>{error}</Alert>}

                    <Box component="form" onSubmit={handleSubmit} sx={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
                        <TextField
                            id="name"
                            label="Your Name"
                            value={name}
                            onChange={e => setName(e.target.value)}
                            required
                            fullWidth
                            autoFocus
                        />
                        <TextField
                            id="signup-email"
                            label="Work Email"
                            type="email"
                            value={email}
                            onChange={e => setEmail(e.target.value)}
                            required
                            fullWidth
                        />
                        <TextField
                            id="signup-password"
                            label="Password (min 8 chars)"
                            type="password"
                            value={password}
                            onChange={e => setPassword(e.target.value)}
                            required
                            fullWidth
                            helperText="At least 8 characters"
                        />
                        <Button
                            type="submit"
                            variant="contained"
                            fullWidth
                            disabled={loading}
                            sx={{
                                mt: 1, py: 1.5, fontWeight: 700,
                                background: 'linear-gradient(90deg, #90caf9 0%, #ce93d8 100%)',
                                color: '#000',
                            }}
                        >
                            {loading ? <CircularProgress size={22} /> : 'Start Free Trial'}
                        </Button>

                        <Typography variant="body2" textAlign="center" color="textSecondary">
                            Already have an account?{' '}
                            <Link to="/login" style={{ color: '#90caf9', textDecoration: 'none', fontWeight: 600 }}>
                                Sign in
                            </Link>
                        </Typography>

                        <Typography variant="caption" textAlign="center" color="textSecondary">
                            By signing up you agree to our Terms of Service and Privacy Policy.
                        </Typography>
                    </Box>
                </CardContent>
            </Card>
        </Box>
    );
}

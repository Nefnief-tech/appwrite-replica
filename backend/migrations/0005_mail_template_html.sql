-- HTML alternative for each auth email. Plain-text bodies remain for clients
-- that prefer them; a template with an empty html_body is sent as text only.

alter table ow_mail_templates add column if not exists html_body text not null default '';

update ow_mail_templates set html_body = case kind
    when 'recovery' then $html$<p>Open this link within an hour to choose a new password:</p><p><a href="{{link}}">{{link}}</a></p><p>If you did not ask for this, ignore this email.</p>$html$
    when 'verification' then $html$<p>Confirm your email address:</p><p><a href="{{link}}">{{link}}</a></p>$html$
    when 'mfa' then $html$<p>Your sign-in code is <strong>{{code}}</strong>. It expires in 10 minutes.</p>$html$
    else '' end
where html_body = '';

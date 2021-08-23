/*
Unfortunately, Namecheap API is not available for every account, even if you buy a single domain.
After Terraform done, go to Namecheap and buy a domain (in my case it was dlemel8.xyz).
In the domain Advanced DNS tab, under PERSONAL DNS SERVER, add 2 name server (ns1 and ns2) with the public address of the created Linode.
In the domain Domain tab, under NAMESERVERS, select Custom DNS and fill the names of name servers.
*/